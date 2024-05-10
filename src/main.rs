//#![allow(warnings)]

//#![allow(dead_code)]

//Libraries
use axum::{
    body::Bytes,
    debug_handler,
    extract::{State, ConnectInfo},
    extract::{DefaultBodyLimit, Query},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Router,
};

use std::{
    convert::Infallible,
    fs::{create_dir_all, File},
    io::prelude::*,
    net::SocketAddr,
    path::PathBuf,
};

use clap::Parser;
use futures_util::stream::once;
use gcp_auth::{AuthenticationManager, CustomServiceAccount};
use google_drive::{traits::FileOps, Client};
use multer::{parse_boundary, Field, Multipart};
use serde::Deserialize;

use tokio::signal;

use std::sync::Arc;

use tokio::sync::RwLock;

//CLI-arguments
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(
        short,
        long,
        help = "Token to be used in authentication",
        default_value = "password"
    )]
    token: String,

    #[arg(short, long, help = "Local port to bind to", default_value_t = 80)]
    port: u16,

    #[arg(
        name = "DATA-FOLDER",
        short = 'd',
        long = "data-folder",
        help = "Folder to to save received files to",
        default_value = "uploads"
    )]
    datafolder: PathBuf,

    #[arg(
        name = "DRIVE-ID",
        short = 'D',
        long = "drive-id",
        help = "ID of drive to upload files to, always end with a space",
        default_value = "null"
    )]
    drive_id: String,

    #[arg(
        name = "PARENT-ID",
        short,
        long = "parent-id",
        help = "ID of folder to upload files to",
        default_value = "null"
    )]
    parent_id: String,

    #[arg(
        name = "IDENTITY-FILE",
        short = 'i',
        long = "identity-file",
        help = "File containing credentials for Google Drive API",
        default_value = "credentials.json"
    )]
    identity_file: PathBuf,
}

//To parse token
#[derive(Debug, Deserialize)]
struct TokenParams {
    token: Option<String>,
}

#[derive(Clone)]
struct ClientState {
    client: Arc<RwLock<Result<Client, &'static str>>>,
}

//Main function
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //Parse CLI-arguments
    let args = Args::parse();

    if args.drive_id == "null" || args.parent_id == "null" {
        panic!("Drive ID or Parent ID was not properly defined, exiting!");
    }

    tracing_subscriber::fmt::init();

    //Try authenticating
    let initialclient: Result<Client, &'static str> = match call_google().await {
        Ok(client) => {
            tracing::info!("Authentication successful!");
            Ok(client)
        }
        Err(e) => {
            //Handle errors
            tracing::error!("Authentication failed!");
            Err(e)
        }
    };

    let global_client = ClientState {
        client: Arc::new(RwLock::new(initialclient)),
    };

    let app = Router::new()
        .route("/", get(root))
        .route("/upload", post(receive_file)) //Upload path
        .layer(DefaultBodyLimit::max(1073741824))//Upload size limit in bytes
        .with_state(global_client);
    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    tracing::info!("listening on {}", addr);
    axum::Server::bind(&addr) //Start server and bind to port
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(_e) = signal::ctrl_c().await {
            tracing::error!("Failed to install SIGINT handler")
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => signal.recv().await,
            Err(_e) => {
                tracing::error!("Failed to install SIGTERM handler");
                None
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("signal received, starting graceful shutdown");
}

async fn call_google() -> Result<Client, &'static str> {
    let args = Args::parse();
    let service_account = match CustomServiceAccount::from_file(&args.identity_file) {
        //Read account file
        Ok(service_account) => service_account,
        Err(_e) => return Err("Error credentials reading file"),
    };
    let authentication_manager = AuthenticationManager::from(service_account);
    let scopes = &["https://www.googleapis.com/auth/drive.file"];
    let token = match authentication_manager.get_token(scopes).await {
        //Acquire token
        Ok(token) => token,
        Err(_e) => return Err("Error acquiring token"),
    };

    let client = Client::new(
        //Creating client using token
        String::from(""),
        String::from(""),
        String::from(""),
        token.as_str(),
        "",
    );

    #[cfg(debug_assertions)]
    client.set_expires_in(200).await;

    Ok(client) //Return Client at success
}

async fn upload_to_drive(
    client: &Client,
    drive: &str,
    folder: &str,
    name: &str,
    mime_type: &str,
    content: &[u8],
) -> Result<(), &'static str> {
    //This function returns null at success or string slice at fail
    match client
        .files()
        .create_or_update(drive, folder, name, mime_type, content)
        .await
    {
        Ok(r) => {
            #[cfg(debug_assertions)]
            {
                tracing::info!(drive);
                tracing::info!("Drive response: {}", r.status)
            }
            return Ok(()); //Null
        }
        Err(_) => {
            return Err("Unable to upload file to Google Drive"); //String slice
        }
    }
}

async fn root() -> &'static str {
    "Hello, World!" //Greet visitors
}

#[debug_handler]
async fn receive_file(
    State(client): State<ClientState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(params): Query<TokenParams>, //Parse queries for authentication
    headers: HeaderMap,                //Parse headers for boundary detection
    body: Bytes,                       //Parse body as bytes
) -> (StatusCode, String) {
    //Always return code and string as tuple
    let args = Args::parse();
    let token = params.token.as_deref().unwrap_or("Null");
    if token != &args.token {
        //authentication
        tracing::info!("Failed authentication from {}", addr);
        return (
            StatusCode::UNAUTHORIZED,
            "Authentication failed".to_string(),
        );
    }

    let mut results: [u8; 3] = [0, 0, 0];//0=success, 1=partial fail, 2=total failure

    let mut fail_handler = |success: [bool; 2], filename: String| {
        match success {
            [true, true] => {
                tracing::info!("Upload of \"{}\" from {} succeeded", filename, addr);
                results[0] += 1;
            },
            [true, false] => {
                tracing::error!("Upload of \"{}\" from {} saved locally, Drive upload failed!", filename, addr);
                results[1] += 1;
            },
            [false, true] => {
                tracing::error!("Upload of \"{}\" from {} saved to Drive, local write failed!", filename, addr);
                results[1] += 1;
            },
            [false, false] => {
                tracing::error!("Upload of \"{}\" from {} failed completely!", filename, addr);
                results[2] += 1;
            },
        }
    };

    let content_type = match headers.get("Content-Type") {
        Some(content_type) => match content_type.to_str() {
            Ok(content_type) => content_type,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    "Header \"Content-Type\" not convertible to string.".to_string(),
                )
            }
        },
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Header \"Content-Type\" not found.".to_string(),
            )
        }
    };

    let boundary = match parse_boundary(content_type) {
        Ok(boundary) => boundary,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Could not extract the boundary from header \"Content-Type\"".to_string(),
            )
        }
    };

    let stream = once(async move { Result::<Bytes, Infallible>::Ok(body) }); //Consume body as streama
    let mut multipart = Multipart::new(stream, boundary);

    let field = match multipart.next_field().await {
        Ok(field) => field,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                "Could not extract field in body".to_string(),
            )
        }
    };

    let field_parts: (String, String); //Declare variable in right scope
    match field {
        Some(field) => {
            match get_field_parts(field).await {
                Ok(f) => field_parts = f,
                Err(e) => {
                    tracing::error!("Error when parsing fields in request from {}: {}", addr, e);
                    return (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to parse fields, no files uploaded!"))
                },
            };
        }
        None => return (StatusCode::BAD_REQUEST, "Failed to read field".to_string()),
    }

    fail_handler(write(field_parts.1, field_parts.0.clone(), &client).await, field_parts.0);

    while let Ok(field) = multipart.next_field().await {
        //Try next field until it returns error
        match field {
            Some(field) => match get_field_parts(field).await {
                Ok(field) => fail_handler(write(field.1, field.0.clone(), &client).await, field.0),
                Err(e) => tracing::error!("Error when parsing fields: {}", e),
            },
            None => break,
        }
    }

    match results {
        [1, 0, 0] => return (StatusCode::OK, "File uploaded successfully.".to_string()),
        [s, 0, 0] => return (StatusCode::OK, format!("{} files uploaded successfully.", s)),
        [s, f, 0] => return (StatusCode::OK, format!("{} files uploaded successfully, {} file upload(s) partially failed", s, f)),
        [0, 0, t] => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{} file upload(s) totally failed", t)),
        [s, 0, t] => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{} file(s) uploaded successfully, {} file upload(s) totally failed", s, t)),
        [0, f, t] => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{} file upload(s) partially failed, {} file uploads totally failed", t, f)),
        [s, f, t] => return (StatusCode::INTERNAL_SERVER_ERROR, format!("{} file(s) uploaded successfully, {} file upload(s) partially failed, {} file upload(s) totally failed", s, t, f)),
    }
}

async fn get_field_parts(field: Field<'_>) -> Result<(String, String), String> {
    //Extract parts from field
    let fieldname: String;
    let fieldtext = {
        fieldname = match field.name() {
            Some(fieldname) => fieldname.to_string(),
            None => {
                return Err("Failed to read field name".to_string())
            }
        };
        match field.text().await {
            Ok(fieldtext) => fieldtext,
            Err(_) => {
                return Err("Failed to read field text".to_string())
            }
        }
    };
    //println!("Name: {}\nContent: {}", fieldname, fieldtext);
    Ok((fieldname, fieldtext))
}

async fn write(
    body: String,
    filename: String,
    clientstate: &ClientState,
) -> [bool; 2] {
    let args = Args::parse();
    let mut success: [bool; 2] = [false, false];

    //Save locally
    success[0] = 'disk: {
        if let Err(e) = create_dir_all(&args.datafolder) {
            println!(
                "Error received when creating directories: {}",
                e.to_string()
            );
            break 'disk false;
        }
        let fullpath = args.datafolder.join(&filename); //Join datafolder and filename to get full path
        let mut file = match File::create(&fullpath) {
            Ok(file) => file,
            Err(e) => {
                println!("Error received: {}", e.to_string());
                break 'disk false;
            }
        };
        if let Err(e) = file.write_all(body.as_bytes()) {
            //Write to disk
            println!("Error received: {}", e.to_string());
            break 'disk false;
        }
        break 'disk true;
    };

    //Save to Drive
    success[1] = 'drive: {

        for _x in 0..2 {
            match clientstate.client.as_ref().read().await.as_ref() {
                Ok(c) => {
                    #[cfg(debug_assertions)] //While testing, print token expiration
                    if let Some(t) = c.expires_in().await {
                        let expiry = format!("{}", t.as_secs());
                        tracing::info!(expiry);
                    };
                    if let Some(false) = c.is_expired().await {
                        //If not expired, try upload
                        break 'drive upload(
                            c,
                            &args.drive_id,
                            &args.parent_id,
                            &filename,
                            body.as_bytes(),
                        )
                        .await;
                    }
                }
                Err(e) => {
                    let s = format!("Could not get value of client: {}", e);
                    tracing::error!(s);
                    break 'drive false;
                }
            }
            update(clientstate).await;
        }

        async fn upload(
            client: &Client,
            drive_id: &str,
            parent_id: &str,
            filename: &str,
            body: &[u8],
        ) -> bool {
            if let Err(e) =
                upload_to_drive(client, drive_id, parent_id, filename, "text/plain", body).await
            {
                let s = format!("Error uploading to drive: {}", e);
                tracing::error!(s);
                return false;
            }
            return true;
        }

        async fn update(clientstate: &ClientState) -> bool {
            let mut c = clientstate.client.write().await;
            match call_google().await {
                Ok(nc) => {
                    tracing::info!("Refreshing authentication successful!");
                    *c = Ok(nc);
                }
                Err(e) => {
                    tracing::error!("Refreshing authentication failed! {:}", e);
                    return false;
                }
            }
            true
        }
        true
    };
    success
}

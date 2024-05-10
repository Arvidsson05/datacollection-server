# DataCollectionServer
This program listens for incoming HTTP connections, authenticates them and recieves files that saves to disk and on Google Drive

# Building
In order to build this program, Rust needs to be installed together with Cargo. Instructions can be found at the [Rust website](https://www.rust-lang.org/learn/get-started). Clone the repo and run the following command in the project root to build:
```
cargo build --release
```

# Running the program
The program can be run using the default parameters using the following command:
```
./target/release/datacollectionserver
```
Stop the program with <kbd>Ctrl</kbd> + <kbd>C</kbd> or by sending it the SIGINT signal.

# Options

| Long flag         | Short flag | Description                                             | Default value                     |
|-------------------|------------|---------------------------------------------------------|-----------------------------------|
| `--token`         | `-t`       | Token to be used in authentication                      | password                          |
| `--port`          | `-p`       | Local port to bind to                                   | 8080                              |
| `--data-folder`   | `-d`       | Folder to save received files to                        | uploads                           |
| `--drive-id`      | `-D`       | ID of drive to upload files to, always end with a space |                                   |
| `--parent-id`     | `-P`       | ID of folder to upload files to                         |                                   |
| `--identity-file` | `-i`       | File containing credentials for Google Drive API        | credentials.json                  |
| `--help`          | `-h`       | Print help                                              |                                   |
| `--version`       | `-V`       | Print version                                           |                                   |

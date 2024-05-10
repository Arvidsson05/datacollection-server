#!/bin/bash

file ./secrets/credentials.json
file secrets/credentials.json
sha256sum ./secrets/credentials.json

(./target/release/datacollectionserver -t testpass -p 1200 -d /tmp -i ./secrets/credentials.json || echo "Server error, test failed!"; exit 1) &

sleep 15;

curl -X POST -H "Content-Type: multipart/form-data; boundary=----------------------------4ebf00fbcf09" -H "file-name: json.json" -d $'------------------------------4ebf00fbcf09\r\nContent-Disposition: form-data; name="example"\r\n\r\ntest\r\n------------------------------4ebf00fbcf09--\r\n' http://localhost:1200/upload?token=testpass || (echo "Client error, test failed!"; kill %1; exit 2)

cat /tmp/example

kill %1

exit 0

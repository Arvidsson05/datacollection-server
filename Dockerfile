# syntax=docker/dockerfile:1

FROM rust:slim-bookworm
WORKDIR ./
COPY . .
RUN cargo install --path . && ln -sf /run/secrets/gcp_credentials /dimfrost-automation.json; \
ln -sf /run/secrets/token /token
CMD ["sh", "-c", "datacollectionserver --token $TOKEN"]

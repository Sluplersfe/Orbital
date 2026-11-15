# Static orbital-server image. Build from the repo root: docker build -f server/Dockerfile .
FROM golang:1.27 AS build
WORKDIR /src
COPY server/go.mod server/go.sum ./
RUN go mod download
COPY server/ ./
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /orbital-server ./cmd/orbital-server

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /orbital-server /orbital-server
WORKDIR /data
EXPOSE 4004/tcp 4005/udp
ENTRYPOINT ["/orbital-server", "-tokens", "/data/tokens.txt"]

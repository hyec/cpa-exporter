FROM golang:1.24-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o /out/cpa-exporter ./cmd/cpa-exporter

FROM alpine:3.22
COPY --from=build /out/cpa-exporter /usr/local/bin/cpa-exporter
EXPOSE 9321
ENTRYPOINT ["/usr/local/bin/cpa-exporter"]

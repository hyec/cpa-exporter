FROM golang:1.24-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o /out/cpa-exporter ./cmd/cpa-exporter
RUN CGO_ENABLED=0 GOOS=linux go build -o /out/fake-cpa ./test/fake-cpa

FROM alpine:3.22 AS exporter
COPY --from=build /out/cpa-exporter /usr/local/bin/cpa-exporter
EXPOSE 9321
ENTRYPOINT ["/usr/local/bin/cpa-exporter"]

FROM alpine:3.22 AS fake-cpa
COPY --from=build /out/fake-cpa /usr/local/bin/fake-cpa
EXPOSE 8317
ENTRYPOINT ["/usr/local/bin/fake-cpa"]

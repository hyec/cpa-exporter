FROM --platform=$BUILDPLATFORM golang:alpine AS build
ARG TARGETARCH TARGETOS
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOOS=$TARGETOS GOARCH=$TARGETARCH go build -o /out/cpa-exporter ./cmd/cpa-exporter

FROM alpine:latest
COPY --from=build /out/cpa-exporter /usr/local/bin/cpa-exporter
EXPOSE 9321
ENTRYPOINT ["/usr/local/bin/cpa-exporter"]

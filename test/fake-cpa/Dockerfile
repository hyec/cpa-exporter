FROM golang:1.24-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY test/fake-cpa ./test/fake-cpa
RUN CGO_ENABLED=0 GOOS=linux go build -o /out/fake-cpa ./test/fake-cpa

FROM alpine:3.22
COPY --from=build /out/fake-cpa /usr/local/bin/fake-cpa
EXPOSE 8317
ENTRYPOINT ["/usr/local/bin/fake-cpa"]

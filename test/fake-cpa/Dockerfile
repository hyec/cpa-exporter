FROM python:3-alpine
COPY test/fake-cpa/main.py /app/main.py
EXPOSE 8317
USER 65532:65532
ENTRYPOINT ["python3", "/app/main.py"]

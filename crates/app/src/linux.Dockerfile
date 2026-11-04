FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc g++ make default-jdk-headless python3 python3-matplotlib nodejs \
    bash vim bc gawk sed grep findutils procps psmisc util-linux file less cron \
    mariadb-server mariadb-client graphviz fonts-dejavu-core ca-certificates \
    && rm -rf /var/lib/apt/lists/*
CMD ["sh", "-c", "service mariadb start >/dev/null 2>&1; service cron start >/dev/null 2>&1; touch /tmp/ready; exec sleep 7200"]

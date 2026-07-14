FROM alpine:3.20

RUN apk add --no-cache openssh-server tar zstd \
    && adduser -D -s /bin/sh backup \
    && mkdir -p /run/sshd /home/backup/.ssh /srv/app \
    && chown -R backup:backup /home/backup /srv/app \
    && chmod 700 /home/backup/.ssh \
    && ssh-keygen -A

COPY payload/ /srv/app/

CMD ["/usr/sbin/sshd", "-D", "-e"]

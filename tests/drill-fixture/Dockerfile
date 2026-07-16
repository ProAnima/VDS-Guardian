FROM alpine:3.20

RUN apk add --no-cache openssh-server tar zstd sqlite coreutils pv \
    && adduser -D -s /bin/sh backup \
    && passwd -d backup \
    && mkdir -p /run/sshd /home/backup/.ssh /srv/app \
    && chown -R backup:backup /home/backup /srv \
    && chmod 700 /home/backup/.ssh

COPY payload/ /srv/app/
RUN sqlite3 /srv/app/app.sqlite "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes (id, body) VALUES (1, 'clean-room drill seed row');" \
    && chown backup:backup /srv/app/app.sqlite

CMD ["/usr/sbin/sshd", "-D", "-e"]

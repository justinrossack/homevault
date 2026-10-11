FROM alpine:3.22
RUN apk add --no-cache samba-server samba-common-tools tini
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
EXPOSE 445
ENTRYPOINT ["/sbin/tini", "--", "/entrypoint.sh"]

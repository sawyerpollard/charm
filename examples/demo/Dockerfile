# Smallest possible "real app" for end-to-end testing: busybox httpd on :8080.
FROM busybox
RUN mkdir -p /www && echo "<h1>charm demo — it works</h1>" > /www/index.html
EXPOSE 8080
CMD ["httpd", "-f", "-p", "8080", "-h", "/www"]

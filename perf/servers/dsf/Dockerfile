FROM debian:latest

# Fetch and install dsf
# TODO: this is a -copy- because otherwise building takes -forever-
COPY dsfd /usr/local/bin/dsfd

# Add entrypoint script
COPY entrypoint.sh /

# Set DSF entrypoint
CMD [ "/entrypoint.sh" ]

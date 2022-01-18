FROM debian:latest

# Fetch dependencies
RUN apt-get update && apt-get install -y \
    mosquitto

COPY mosquitto.conf /etc/mosquitto/mosquitto.conf

# Set mosquitto entrypoint
CMD [ "/usr/sbin/mosquitto", "-c", "/etc/mosquitto/mosquitto.conf" ]

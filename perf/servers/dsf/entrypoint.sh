#!/bin/bash

# Create DSF directory
mkdir -p /etc/dsf

# Setup DSF config options
export DSF_DB_FILE=/run/dsfd.db
export DSF_SOCK=/var/dsfd.sock

# Limit to one thread for fairness
export ASYNC_STD_THREAD_COUNT=1

# Run DSF
/usr/local/bin/dsfd --no-bootstrap

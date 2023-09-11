#!/bin/bash

N=$1
ADDR=192.168.4.0:10100

for (( i=1; i<$N; i++ )); do
    echo "Connect set $i to addr $ADDR"
    kubectl exec -it pod/dsf-set-$i -- /usr/local/bin/dsfc peer connect $ADDR
done


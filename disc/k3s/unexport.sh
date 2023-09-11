#!/bin/bash

N=$1

for (( i=0; i<$N; i++ )); do

    kubectl delete service dsf-svc-$i
done


#!/bin/bash

N=$1

for (( i=0; i<$N; i++ )); do

CFG="
---
apiVersion: v1
kind: Service
metadata:
  name: dsf-svc-$i
  annotations:
    metallb.universe.tf/address-pool: l2-pool
spec:
  selector:
    statefulset.kubernetes.io/pod-name: dsf-set-$i
  type: LoadBalancer
  externalTrafficPolicy: Local
  ports:
    - name: dsf-udp
      protocol: UDP
      port: 10100
      targetPort: 10100
    - name: dsf-http
      protocol: TCP
      port: 10180
      targetPort: 10180
"

    echo "$CFG" | kubectl apply -f -

done


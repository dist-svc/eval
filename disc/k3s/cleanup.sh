#!/bin/bash

kubectl delete statefulset.apps/dsf-set
kubectl delete pod --all
kubectl delete pvc --all
kubectl delete pv --all


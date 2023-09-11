#!/bin/bash

 N=$1

 kubectl scale --replicas=$N statefulset.apps/dsf-set

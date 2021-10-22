#!/bin/bash

FILE=$1
COUNT=$(jq length $1)

echo "Parsing $COUNT values from $FILE"

JSON_TOT=$((0))
CBOR_TOT=$((0))

for D in $(cat $FILE | jq -c '.[]'); do

# Calculate JSON length
JSON=$(echo $D | wc -c)

# Calculate CBOR length
CBOR=$(echo $D | ./target/debug/cbor-util to-cbor | wc -c)


echo "Data: $D"
echo "JSON bytes: $JSON, CBOR bytes: $CBOR"

JSON_TOT=$(($JSON_TOT + $JSON)) 
CBOR_TOT=$(($CBOR_TOT + $CBOR)) 

done

echo "Totals"
echo "JSON bytes: $JSON_TOT, CBOR bytes: $CBOR_TOT"


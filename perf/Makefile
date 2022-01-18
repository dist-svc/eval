

images: image-coap image-mqtt image-dsf

image-coap:
	docker build -t iot-perf-coap servers/coap/

image-mqtt:
	docker build -t iot-perf-mqtt servers/mqtt

image-dsf:
	docker build -t iot-perf-dsf servers/dsf
	
run-dsf:
	cargo run --release -- --docker-mode=http --target=${TARGET} --log-level=info --mode-filters=dsf --output=results/results-dsf.json
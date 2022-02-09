

images: image-coap image-mqtt image-dsf

image-coap:
	docker buildx build --platform linux/arm64 -t ghcr.io/ryankurte/dsf-iot-eval/iot-perf-coap --push servers/coap

image-mqtt:
	docker buildx build --platform linux/arm64 -t ghcr.io/ryankurte/dsf-iot-eval/iot-perf-mqtt --push servers/mqtt

image-dsf:
	docker buildx build --platform linux/arm64 -t ghcr.io/ryankurte/dsf-iot-eval/iot-perf-dsf --push servers/dsf
	
run-dsf:
	cargo run --release -- --docker-mode=http --target=${TARGET} --log-level=info --mode-filters=dsf --output=results/results-dsf.json
	
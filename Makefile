#signing_bot/target/x86_64-unknown-linux-musl/release/signing_bot:
#	cd signing_bot; cargo build --release --target=x86_64-unknown-linux-musl

bot: # signing_bot/target/x86_64-unknown-linux-musl/release/signing_bot
	cd signing_bot; cargo build --release --target=x86_64-unknown-linux-musl

resources/lambda/bootstrap: bot
	cp signing_bot/target/x86_64-unknown-linux-musl/release/signing_bot resources/lambda/bootstrap

build: resources/lambda/bootstrap
	npm install
	cdk synth

deploy: build
	cdk deploy

clean:
	rm resources/lambda/bootstrap || true
	cd signing_bot; cargo clean
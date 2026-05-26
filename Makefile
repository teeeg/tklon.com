# Deploy + local dev for tklon.com
# Requires: aws CLI (authenticated, e.g. `aws login`), rbenv, node.
STACK  ?= tklondotcom
REGION ?= us-east-1
TEMPLATE := deploy/template.yml

# Run gem/middleman via the project's pinned Ruby (src/.ruby-version),
# even if rbenv isn't initialised in the calling shell.
RUBYPATH := PATH="$$HOME/.rbenv/shims:$$PATH"

.DEFAULT_GOAL := help
.PHONY: help install images video video-prune build serve test infra publish deploy

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
	  awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-9s\033[0m %s\n", $$1, $$2}'

install: ## Install Ruby gems + npm deps
	cd src && $(RUBYPATH) bundle install && npm install

images: ## Regenerate responsive image variants from src/images/ (also run automatically by build/serve)
	cd src && node scripts/build-images.mjs

video: ## Encode + upload a self-hosted video (manual; needs ffmpeg). Usage: make video SRC=src/videos/foo.mov
	cd src && node scripts/build-videos.mjs $(SRC)

video-prune: ## Delete /media/ objects on S3 not referenced by data/videos.json (opt-in GC)
	cd src && node scripts/build-videos.mjs --prune

build: ## Build the static site into src/build
	cd src && $(RUBYPATH) NO_CONTRACTS=true bundle exec middleman build

serve: ## Run the local dev server (http://localhost:4567)
	cd src && $(RUBYPATH) bundle exec middleman server

test: ## Run the JS (vitest) suite
	cd src && npm test

infra: ## Deploy/update the CloudFormation stack
	aws cloudformation deploy \
	  --template-file $(TEMPLATE) \
	  --stack-name $(STACK) \
	  --capabilities CAPABILITY_IAM CAPABILITY_AUTO_EXPAND \
	  --region $(REGION)

publish: build ## Build, sync to S3, and invalidate CloudFront (content deploy)
	@bucket=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id WebsiteStaticAssetsBucket \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)); \
	dist=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id CloudfrontDistribution \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)); \
	echo "→ syncing src/build to s3://$$bucket, then invalidating $$dist"; \
	aws s3 sync src/build/ "s3://$$bucket" --delete --exclude 'media/*'; \
	aws cloudfront create-invalidation --distribution-id "$$dist" --paths '/*'

deploy: infra publish ## Full deploy: stack update, then content

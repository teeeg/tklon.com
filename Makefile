# Deploy + local dev for tklon.com
# Requires: aws CLI (authenticated, e.g. `aws login`), rbenv, node.
STACK  ?= tklondotcom
REGION ?= us-east-1
TEMPLATE := deploy/template.yml
# Holds zipped Lambda code uploaded by `cloudformation package`. Created
# idempotently by `infra` — no manual bootstrap needed.
ARTIFACTS_BUCKET ?= $(STACK)-cfn-artifacts
PACKAGED_TEMPLATE := deploy/template.packaged.yml

# Run gem/middleman via the project's pinned Ruby (src/.ruby-version),
# even if rbenv isn't initialised in the calling shell.
RUBYPATH := PATH="$$HOME/.rbenv/shims:$$PATH"

.DEFAULT_GOAL := help
.PHONY: help install install-hooks images video video-prune check-videos build serve test infra publish deploy

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
	  awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-13s\033[0m %s\n", $$1, $$2}'

install: install-hooks ## Install Ruby gems + npm deps (and git hooks)
	cd src && $(RUBYPATH) bundle install && npm install

install-hooks: ## Point git at .githooks/ so pre-push runs check-videos
	git config core.hooksPath .githooks

images: ## Regenerate responsive image variants from src/images/ (also run automatically by build/serve)
	cd src && node scripts/build-images.mjs

video: ## Encode + upload a self-hosted video (manual; needs ffmpeg). Usage: make video SRC=src/videos/foo.mov
	cd src && node scripts/build-videos.mjs $(SRC)

video-prune: ## Delete /media/ objects on S3 not referenced by data/videos.json (opt-in GC)
	cd src && node scripts/build-videos.mjs --prune

check-videos: ## Verify every src/videos/ source is reflected in data/videos.json (run by pre-push)
	@cd src && node scripts/check-videos.mjs

build: ## Build the static site into src/build
	cd src && $(RUBYPATH) NO_CONTRACTS=true bundle exec middleman build

serve: ## Run the local dev server (http://localhost:4567)
	cd src && $(RUBYPATH) bundle exec middleman server

test: ## Run the JS (vitest) suite
	cd src && npm test

infra: ## Deploy/update the CloudFormation stack (also packages Lambda code)
	@aws s3api head-bucket --bucket $(ARTIFACTS_BUCKET) --region $(REGION) 2>/dev/null \
	  || aws s3api create-bucket --bucket $(ARTIFACTS_BUCKET) --region $(REGION) >/dev/null
	aws cloudformation package \
	  --template-file $(TEMPLATE) \
	  --s3-bucket $(ARTIFACTS_BUCKET) \
	  --output-template-file $(PACKAGED_TEMPLATE) \
	  --region $(REGION)
	aws cloudformation deploy \
	  --template-file $(PACKAGED_TEMPLATE) \
	  --stack-name $(STACK) \
	  --capabilities CAPABILITY_IAM CAPABILITY_AUTO_EXPAND \
	  --region $(REGION)

publish: build ## Build, sync to S3 with cache headers, invalidate HTML on CloudFront
	@bucket=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id WebsiteStaticAssetsBucket \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)); \
	dist=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id CloudfrontDistribution \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)); \
	echo "→ syncing hashed/immutable assets to s3://$$bucket"; \
	aws s3 sync src/build/ "s3://$$bucket" --delete --exclude 'media/*' \
	  --exclude '*.html' --exclude '*.xml' --exclude '*.txt' \
	  --cache-control 'public,max-age=31536000,immutable'; \
	echo "→ syncing HTML / feeds (short TTL with stale-while-revalidate)"; \
	aws s3 sync src/build/ "s3://$$bucket" --delete --exclude '*' \
	  --include '*.html' --include '*.xml' --include '*.txt' \
	  --cache-control 'public,max-age=60,s-maxage=300,stale-while-revalidate=86400'; \
	echo "→ invalidating $$dist (HTML/feeds only — hashed assets self-bust)"; \
	aws cloudfront create-invalidation --distribution-id "$$dist" \
	  --paths '/*.html' '/*.xml' '/*.txt'

deploy: infra publish ## Full deploy: stack update, then content

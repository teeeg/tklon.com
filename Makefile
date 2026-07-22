# AWS layer for tklon.com: infra, publish, deploy, stats.
#
# The site itself is built by the `tklon` binary, not make — see the README:
#   cargo run -- build      cargo run -- serve
#
# Requires: aws CLI (authenticated, e.g. `aws login`), cargo, and uv (for stats).
STACK  ?= tklondotcom
REGION ?= us-east-1
TEMPLATE := deploy/template.yml
# Customizable values live in params.yml — edit there, not in the template
# (which is generic schema). See deploy/params.yml for the format.
PARAMS := deploy/params.yml
# Holds zipped Lambda code uploaded by `cloudformation package`. Created
# idempotently by `infra` — no manual bootstrap needed.
ARTIFACTS_BUCKET ?= $(STACK)-cfn-artifacts
PACKAGED_TEMPLATE := deploy/template.packaged.yml

.DEFAULT_GOAL := help
.PHONY: help fmt infra publish deploy stats

help: ## List available targets
	@grep -hE '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
	  awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-13s\033[0m %s\n", $$1, $$2}'

fmt: ## Format Python (ruff) and Rust (cargo fmt)
	@command -v uvx >/dev/null || { echo "uv not installed — see \`make stats\` for a hint"; exit 1; }
	uvx ruff format deploy/lambda/
	cargo fmt

infra: ## Deploy/update the CloudFormation stack (also packages Lambda code)
	@aws s3api head-bucket --bucket $(ARTIFACTS_BUCKET) --region $(REGION) 2>/dev/null \
	  || aws s3api create-bucket --bucket $(ARTIFACTS_BUCKET) --region $(REGION) \
	       $$(test "$(REGION)" = us-east-1 || echo --create-bucket-configuration LocationConstraint=$(REGION)) \
	       >/dev/null
	aws cloudformation package \
	  --template-file $(TEMPLATE) \
	  --s3-bucket $(ARTIFACTS_BUCKET) \
	  --output-template-file $(PACKAGED_TEMPLATE) \
	  --region $(REGION)
	aws cloudformation deploy \
	  --template-file $(PACKAGED_TEMPLATE) \
	  --stack-name $(STACK) \
	  --parameter-overrides $$(sed -nE -e 's/[[:space:]]*\r?$$//' -e 's/^([A-Za-z_][A-Za-z0-9_]*):[[:space:]]+(.+)$$/\1=\2/p' $(PARAMS)) \
	  --capabilities CAPABILITY_IAM CAPABILITY_AUTO_EXPAND \
	  --region $(REGION)

publish: ## Build the site with tklon, sync to S3 with cache headers, invalidate HTML
	cargo run --release -- build
	@bucket=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id WebsiteStaticAssetsBucket \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)); \
	dist=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id CloudfrontDistribution \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)); \
	echo "→ syncing hashed/immutable assets to s3://$$bucket"; \
	aws s3 sync build/ "s3://$$bucket" --delete --exclude 'media/*' \
	  --exclude '*.html' --exclude '*.xml' --exclude '*.txt' \
	  --cache-control 'public,max-age=31536000,immutable'; \
	echo "→ syncing HTML / feeds (short TTL with stale-while-revalidate)"; \
	aws s3 sync build/ "s3://$$bucket" --delete --exclude '*' \
	  --include '*.html' --include '*.xml' --include '*.txt' \
	  --cache-control 'public,max-age=60,s-maxage=300,stale-while-revalidate=86400'; \
	echo "→ invalidating $$dist (HTML/feeds only — hashed assets self-bust)"; \
	aws cloudfront create-invalidation --distribution-id "$$dist" \
	  --paths '/*.html' '/*.xml' '/*.txt'

deploy: infra publish ## Full deploy: stack update, then content

stats: ## Print access-log report. Override window: WINDOW=12w (default 7d). Units: Nd/Nw/Nmo/Ny/all
	@command -v uv >/dev/null || { echo "uv not installed — run \`brew install uv\` (or see https://docs.astral.sh/uv/getting-started/installation/)"; exit 1; }
	@LOGS_BUCKET=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id WebsiteLogsBucket \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)) \
	 STATS_BUCKET=$$(aws cloudformation describe-stack-resource --stack-name $(STACK) \
	  --logical-resource-id WebsiteStatsBucket \
	  --query 'StackResourceDetail.PhysicalResourceId' --output text --region $(REGION)) \
	 AWS_DEFAULT_REGION=$(REGION) \
	 uv run deploy/lambda/stats_report.py --local --window $(or $(WINDOW),7d)

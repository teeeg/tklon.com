###
# Page options, layouts, aliases and proxies
###

# Per-page layout changes:
#
# With no layout
page '/*.xml', layout: false
page '/*.json', layout: false
page '/*.txt', layout: false
# page "/path/to/file.html", layout: :otherlayout

page '/404.html', directory_index: false

# ignore typescript files
ignore '/**/*.ts'

# With alternative layout
# Proxy pages (http://middlemanapp.com/basics/dynamic-pages/)
# proxy "/this-page-has-no-template.html", "/template-file.html", locals: {
#  which_fake_page: "Rendering a fake page with a local variable" }

# General configuration

# Reload the browser automatically whenever files change
configure :development do
  # add gem 'middleman-livereload'
  # activate :livereload
end

###
# Helpers
###

# Methods defined in the helpers block are available in templates
# helpers do
#   def some_helper
#     "Helping"
#   end
# end

# Build-specific configuration
configure :build do
  # expire cached assets on update
  activate :asset_hash
  activate :minify_css
  activate :minify_html
  activate :minify_javascript
end

# compile *.ts -> *.js
activate :external_pipeline,
  name: :webpack,
  command: build? ? "npm run build" : "npm run build -- --watch",
  source: ".tmp/",
  latency: 1

# rename my-blog-post.html -> /my-blog-post/index.html
activate :directory_indexes

###
# Blog settings
###

activate :blog do |blog|
  # This will add a prefix to all links, template references and source paths
  # blog.prefix = "blog"
  blog.permalink = "posts/{title}-{day}{month}{year}.html"
  # Matcher for blog source files
  blog.sources = "posts/{year}/{title}.html"
  blog.taglink = "tags/{tag}.html"
  blog.layout = "layouts/post"
  # blog.year_link = "{year}.html"
  # blog.month_link = "{year}/{month}.html"
  # blog.day_link = "{year}/{month}/{day}.html"
#   blog.default_extension = ".markdown"
  blog.summary_length = nil
  # blog.tag_template = "tag.html"
  # blog.calendar_template = "calendar.html"
  # Enable pagination
  blog.paginate = true
  # blog.per_page = 2
  # blog.page_link = "page/{num}"
end
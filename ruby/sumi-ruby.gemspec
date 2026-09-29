# frozen_string_literal: true

require_relative "lib/sumi/ruby/version"

Gem::Specification.new do |spec|
  spec.name = "sumi-ruby"
  spec.version = Sumi::Ruby::VERSION
  spec.authors = ["Junya Ishihara"]
  spec.summary = "The sumi CLI, which converts PDFs to grayscale or monochrome, packaged as a gem"
  spec.description = <<~DESCRIPTION
    Ships the prebuilt sumi executable for each platform, so that `bundle install` installs the
    same version of sumi in development, CI and production. sumi converts the colors of a PDF
    to grayscale or monochrome while keeping text and vector graphics.
  DESCRIPTION
  spec.homepage = "https://github.com/champierre/sumi"
  spec.license = "MIT"
  spec.required_ruby_version = ">= 3.1"
  spec.metadata = {
    "homepage_uri" => spec.homepage,
    "source_code_uri" => "https://github.com/champierre/sumi/tree/main/ruby",
    "changelog_uri" => "https://github.com/champierre/sumi/releases",
    "bug_tracker_uri" => "https://github.com/champierre/sumi/issues"
  }

  # The executable for each platform is added by the Rakefile when a native gem is built.
  spec.files = Dir["lib/**/*.rb", "exe/sumi", "LICENSE", "README.md"]
  spec.bindir = "exe"
  spec.executables = ["sumi"]
  spec.require_paths = ["lib"]
end

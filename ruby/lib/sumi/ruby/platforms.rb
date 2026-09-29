# frozen_string_literal: true

module Sumi
  module Ruby
    # Gem platforms with a native gem, and the Rust target whose release archive supplies the
    # executable. The Linux executables are statically linked against musl, so the same binary
    # runs on both glibc and musl systems.
    PLATFORMS = {
      "x86_64-linux-gnu" => "x86_64-unknown-linux-musl",
      "x86_64-linux-musl" => "x86_64-unknown-linux-musl",
      "aarch64-linux-gnu" => "aarch64-unknown-linux-musl",
      "aarch64-linux-musl" => "aarch64-unknown-linux-musl",
      "x86_64-darwin" => "x86_64-apple-darwin",
      "arm64-darwin" => "aarch64-apple-darwin",
      "x64-mingw-ucrt" => "x86_64-pc-windows-msvc"
    }.freeze
  end
end

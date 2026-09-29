# sumi-ruby

[sumi](https://github.com/champierre/sumi)（PDF の色をグレースケールや白黒に変換する CLI）のビルド済み実行ファイルを同梱した gem です。
`bundle install` だけで sumi が入り、`Gemfile.lock` で開発環境・CI・本番のバージョンがそろいます。

gem のバージョンは、同梱している sumi のバージョンと同じです。

## インストール

```ruby
# Gemfile
gem "sumi-ruby"
```

```bash
bundle install
```

## 使い方

```bash
bundle exec sumi input.pdf -o output.pdf
```

Ruby からは `Sumi::Ruby.executable` で実行ファイルの絶対パスを取得できます。

```ruby
require "open3"
require "sumi/ruby"

_stdout, stderr, status = Open3.capture3(
  Sumi::Ruby.executable, "input.pdf", "-o", "output.pdf", "--report", "json"
)
```

オプションや JSON レポートの形式は、[sumi の README](https://github.com/champierre/sumi#cli) を参照してください。

## 対応プラットフォーム

| gem のプラットフォーム | 同梱している実行ファイル |
|---|---|
| `x86_64-linux-gnu` `x86_64-linux-musl` | `sumi-x86_64-unknown-linux-musl` |
| `aarch64-linux-gnu` `aarch64-linux-musl` | `sumi-aarch64-unknown-linux-musl` |
| `x86_64-darwin` | `sumi-x86_64-apple-darwin` |
| `arm64-darwin` | `sumi-aarch64-apple-darwin` |
| `x64-mingw-ucrt` | `sumi-x86_64-pc-windows-msvc` |

実行ファイルは [GitHub Releases](https://github.com/champierre/sumi/releases) で配布しているものと同じです。
Linux 版は静的リンク（musl）なので、glibc の環境でも musl（Alpine など）の環境でも動きます。
Linux 向けの gem には RubyGems 3.3.22 以降が必要です。Ruby 3.1.3 以降に付属する RubyGems なら満たしています（それより前の Ruby では `gem update --system` で更新してください）。

### `Gemfile.lock` のプラットフォーム

Bundler は `Gemfile.lock` の `PLATFORMS` にあるプラットフォームの gem を選びます。
開発に使う Mac と本番の Linux のように環境が分かれる場合は、使うプラットフォームを追加しておきます。

```bash
bundle lock --add-platform x86_64-linux-gnu aarch64-linux-gnu arm64-darwin
```

`PLATFORMS` に `ruby` しかないと、実行ファイルを含まない gem が入り、実行時にエラーになります。

### 対応していない環境

上の表にない環境では、sumi をソースからビルドし、そのディレクトリを環境変数 `SUMI_INSTALL_DIR` で指定してください。
`SUMI_INSTALL_DIR` を指定すると、同梱の実行ファイルより優先されます。

```bash
cargo install --git https://github.com/champierre/sumi sumi-cli
export SUMI_INSTALL_DIR="$HOME/.cargo/bin"
```

## 開発

```bash
cd ruby
bundle install
bundle exec rake test          # テスト
bundle exec rake gem:local     # この環境向けの gem を cargo build --release からビルド（pkg/ に出力）
bundle exec rake gem:release[../dist]   # リリースのアーカイブからすべての gem をビルド
```

## ライセンス

[MIT](https://github.com/champierre/sumi/blob/main/LICENSE)

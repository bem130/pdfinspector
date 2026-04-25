# pdfinspector

Rust 製の PDF 構造調査 CLI です。

PDF を静的に解析して、ページ構造、ストリーム、画像、フォント、埋め込みファイル、描画命令の傾向を確認できます。調査用途を主眼にしており、PDF レンダラや完全準拠パーサではありません。

## 構成

- コアライブラリ: `src/lib.rs`
  `no_std + alloc`
- CLI フロントエンド: `src/main.rs`
  `std`

## 現在できること

- PDF バージョン、間接オブジェクト数、ストリーム数、ページ数の表示
- ページごとの `MediaBox`、テキスト有無、演算子数の表示
- テキスト、グラフィックス状態、パス構築、塗り/描画、XObject、色/状態変更の演算子集計
- ページごとのフォント一覧と埋め込みフォント判定
- ページごとの画像 XObject 一覧
- 埋め込みファイル `EmbeddedFile` の検出
- `--objects` / `--streams` / `--all` による全オブジェクト・全ストリーム一覧表示
- `--dump-stream` によるデコード済みストリーム内容の確認
- `--extract` による画像、フォント、埋め込みファイル、全ストリームのフォルダ展開
- `--export-svg` によるベクタ主体ページの簡易 SVG 書き出し

## 対応しているデコード

- `FlateDecode`

次は識別だけ行います。

- `DCTDecode`
- `JPXDecode`
- `JBIG2Decode`
- `CCITTFaxDecode`

これらは画像種別やフィルタ名は表示できますが、現状は内部展開しません。

## ビルド

```powershell
cargo build
```

## 使い方

通常の解析:

```powershell
cargo run -- "C:\path\to\file.pdf"
```

ビルド済みバイナリを直接使う場合:

```powershell
target\debug\pdfinspector.exe "C:\path\to\file.pdf"
```

全オブジェクト・全ストリームも表示:

```powershell
cargo run -- --all "C:\path\to\file.pdf"
```

デコード済みストリームを表示:

```powershell
cargo run -- --dump-stream "2 0" "C:\path\to\file.pdf"
```

PDF の中身を指定フォルダへ展開:

```powershell
cargo run -- --extract outdir "C:\path\to\file.pdf"
```

ベクタ主体ページを SVG に書き出し:

```powershell
cargo run -- --export-svg svg_out "C:\path\to\file.pdf"
```

抽出と SVG を同時に実行:

```powershell
cargo run -- --extract outdir --export-svg svg_out "C:\path\to\file.pdf"
```

## `--extract` の出力

`--extract outdir file.pdf` を実行すると、概ね次の構成で出力します。

```text
outdir/
  manifest.json
  streams/
    2_0.bin
    2_0.decoded.bin
  images/
    18_0.jpg
  fonts/
    9_0.ttf
  embedded_files/
    21_0.bin
```

出力内容:

- `streams/`
  すべてのストリームの生データ
- `streams/*.decoded.bin`
  デコード可能だったストリームの展開後データ
- `images/`
  `Image XObject`
- `fonts/`
  `FontFile`, `FontFile2`, `FontFile3`
- `embedded_files/`
  `EmbeddedFile`
- `manifest.json`
  ページ数、演算子統計、埋め込みファイル要約

## `--export-svg` の対象

SVG 出力は、主に次のようなパス描画中心の PDF を対象にしています。

- `m`, `l`, `c`, `h`, `re`
- `S`, `s`, `f`, `F`, `B`, `b`
- `cm`, `q`, `Q`
- 基本的な色指定 `G/g`, `RG/rg`
- 線幅 `w`

このため、文字フォント描画ではなく図形として作られた PDF は比較的 SVG 化しやすいです。

## 制限事項

- 間接オブジェクトはファイル本体から直接走査しており、`xref table` / `xref stream` / `object stream` への完全対応はまだありません。
- PDF 全仕様を網羅するものではありません。
- `FlateDecode` 以外の圧縮・符号化は現状ほぼ未対応です。
- SVG 出力は最小実装です。
  文字再現、クリッピング、透明度、Form XObject、複雑なグラフィックス状態、OCG、パターン、シェーディングなどは十分に変換できません。
- `--extract` は生データ保存を優先しています。拡張子は推定で付けているため、常に完全ではありません。
- 環境によっては `cargo test` 実行ファイルがアプリケーション制御ポリシーでブロックされることがあります。この環境では `cargo check` と `cargo test --no-run` までは確認済みです。

## ライセンス

このリポジトリは MIT ライセンスです。詳細は [LICENSE](./LICENSE) を参照してください。

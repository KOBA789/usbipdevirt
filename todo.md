# Refactoring TODO

- [x] 1. `UsbipBridge` が `UsbipConnection` のプロトコルロジックを重複実装 → `usbip` crate 側で `UsbipWriter`/`UsbipReader` + `into_split()` をサポート、`main.rs` の生バイト操作を排除
- [x] 2. `ep0_write` が `assert!` でパニック → `io::Result` でエラーを返す
- [x] 3. `ep_write` / `ep_read` のバッファ確保パターン重複 → `EpIoBuf` ヘルパー抽出
- [x] 4. イベントタイプがマジックナンバー → `USB_RAW_EVENT_*` 定数化
- [x] 5. Disconnect / Reset ハンドラが同一 → `Event::Disconnect | Event::Reset` で統合
- [x] 6. `UsbipConnection` から stream を消費的に取り出せない → `into_split()` で #1 と統合
- [x] 7. `Ep0IoBuf` が2回定義 → メソッド外に一度だけ定義
- [x] 8. `let _ = seqnum;` が意図不明 → #1 の書き換えで除去済み
- [x] 9. `protocol.rs` に `read_i32_be` がない → 追加して `UsbipReader::recv` で使用

# VouchFlow

Aplikasi desktop berbasis Rust + Slint untuk manajemen dan transaksi voucher dengan API gateway lokal.

Project ini menggabungkan:
- UI desktop untuk operasional harian (dashboard, produk, stok, utilitas, logs, settings)
- API lokal (`Axum`) untuk menerima request transaksi
- SQLite sebagai penyimpanan utama
- Arsitektur event-driven dengan single-writer untuk menjaga konsistensi data

## Fitur Utama

- Transaksi voucher kategori `CEK`, `RDM`, dan `FIS`.
- Manajemen master data produk.
- Manajemen stok voucher aktif/terpakai.
- Monitoring transaksi real-time di dashboard.
- Logs terminal dengan filter level dan pause.
- Settings server + webhook dari UI.
- Start/stop API server langsung dari sidebar.

Provider yang tersedia saat ini:
- ByU
- Smartfren
- Telkomsel

## Arsitektur Singkat

Arsitektur utama mengacu pada pola berikut:

1. `Gateway (Axum)` menerima request transaksi.
2. Request dikirim ke `Command Bus`.
3. `Orchestrator` menjalankan business flow transaksi.
4. Orchestrator mengirim operasi tulis ke `DbCommand Queue`.
5. `DB Writer` (single-writer) melakukan write ke SQLite.
6. Setelah commit, event dipublish ke `Event Bus`.
7. `Central Store` meng-update read model in-memory.
8. `UI Bridge` mendorong update ke Slint UI dengan aman (`invoke_from_event_loop`).

Tujuan pola ini:
- Menghindari race condition saat write DB.
- Menjaga UI tetap responsif.
- Memisahkan jalur write (command) dan read (UI model).

## Struktur Folder

Berikut struktur penting project:

- `src/main.rs`: entry point aplikasi desktop + wiring komponen.
- `src/config.rs`: konfigurasi aplikasi (default runtime config).
- `src/application/`: orchestrator, service bisnis, provider router, central store.
- `src/domain/`: command, event, model, error domain.
- `src/infrastructure/`: database, channel bus, provider client.
- `src/presentation/gateway/`: HTTP gateway (Axum route + handler).
- `src/presentation/ui/`: bridge event ke UI.
- `src/callbacks/`: callback Slint untuk tiap modul halaman.
- `ui/`: file Slint (window, pages, components, global state).
- `assets/`: aset icon/logo.

## Prasyarat

- Rust toolchain (stable).
- OS yang mendukung build Rust + Slint (project ini umum dipakai di Windows).
- Koneksi jaringan jika provider API eksternal dipanggil.

Cek toolchain:

```bash
rustc --version
cargo --version
```

## Menjalankan Aplikasi

Mode development:

```bash
cargo run
```

Mode release:

```bash
cargo run --release
```

Build binary release:

```bash
cargo build --release
```

Output binary (Windows):

```text
target/release/vouchflow.exe
```

## Alur Penggunaan Singkat

1. Jalankan aplikasi.
2. Buka menu `Settings` untuk cek alamat server/webhook.
3. Tekan tombol `Jalankan Server` di sidebar.
4. Isi master `Produk` dan `Stok Voucher`.
5. Kirim request transaksi ke endpoint API lokal.
6. Pantau hasil di `Dashboard` dan `Logs Terminal`.

## Endpoint API

Base URL default:

```text
http://127.0.0.1:8080
```

Health check:

```http
GET /health
```

Transaksi:

```http
GET /api/v1/transaksi?idtrx=<ID_TRX>&nomor=<NOMOR>&produk=<KODE_PRODUK>
```

Contoh:

```bash
curl "http://127.0.0.1:8080/api/v1/transaksi?idtrx=TRX001&nomor=08123456789&produk=RDM_XYZ"
```

Cek status by request id:

```http
GET /api/v1/status/{request_id}
```

Catatan:
- Produk akan dicari dari tabel `produk` berdasarkan `kode_produk` aktif.
- Jika proses transaksi melebihi timeout sinkron, response dapat kembali sebagai status pending.
- Endpoint status saat ini masih placeholder sederhana.

## Konfigurasi

Konfigurasi default runtime di `src/config.rs`:

- `db_path`: `voucher.db`
- `server_host`: `127.0.0.1`
- `server_port`: `8080`
- `terminal_host`: `127.0.0.1`
- `terminal_port`: `8081`
- `command_bus_capacity`: `100`
- `db_command_capacity`: `100`
- `event_bus_capacity`: `1000`

Konfigurasi operasional juga disimpan di tabel `configurations` dan diubah lewat halaman `Settings`.

## Database

SQLite dipakai sebagai storage utama. Tabel inti:

- `produk`
- `stok_voucher`
- `transactions`
- `transaction_logs`
- `logs`
- `configurations`
- `migrations`

File database default berada di root project:

```text
voucher.db
```

## Logging

Tracing memakai `tracing` + `tracing-subscriber`.

Default filter:

```text
info,vouchflow=debug
```

Override level log:

```bash
$env:RUST_LOG="debug"; cargo run
```

## Pengembangan

Perintah yang umum dipakai:

```bash
cargo fmt
cargo check
cargo test
```

Jika Anda menambah modul UI:
- Definisikan state/callback di `ui/state.slint`.
- Hubungkan callback di `src/callbacks/`.
- Pastikan update UI dari thread async tetap lewat `slint::invoke_from_event_loop`.

## Catatan Implementasi

- API server bersifat lazy-start, tidak otomatis aktif saat aplikasi dibuka.
- Terdapat crash recovery saat startup untuk membersihkan status transaksi/stok yang tertinggal.
- Retensi log dijalankan berkala (default 30 hari, interval 1 jam).

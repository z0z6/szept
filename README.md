# Komunikator

Szyfrowany komunikator tekstowy P2P: bez kont, bez numeru telefonu i bez serwera przechowującego wiadomości. Nazwa jest robocza.

**Stan: kamień milowy 1.** Działa rdzeń i klient terminalowy do rozmów 1:1. To wersja deweloperska, jeszcze nie do poufnej komunikacji (patrz „Bezpieczeństwo”).

## Szybki start

Na obu komputerach:

```
komunikator init                  # tworzy tożsamość i wypisuje identyfikator
komunikator add ola <ID_OLI>      # identyfikator przekaż zaufanym kanałem
komunikator chat ola
```

Wiadomość napisana, gdy kontakt jest offline, czeka w kolejce i wychodzi po jego powrocie. Kolejka żyje na razie tylko do zamknięcia programu.

### Tryb lokalny, bez żadnych zewnętrznych serwerów

```
komunikator chat ola --local                           # wypisze adresy IP:PORT
komunikator chat ala --local --direct 192.168.1.20:PORT
```

Opcję `--direct` podaje jedna strona. Druga czeka na połączenie.

## Sieć i „brak serwera”

- Urządzenia łączą się bezpośrednio przez QUIC ([iroh](https://github.com/n0-computer/iroh)), adresowane kluczem publicznym.
- W trybie domyślnym pomagają dwa rodzaje publicznych serwerów projektu iroh: wyszukiwanie adresu po kluczu oraz przekaźnik (relay) dla sieci, których NAT nie da się przebić. Widzą, kto z kim się łączy, nie widzą treści. Oba można zastąpić własnymi.
- W trybie `--local` program nie kontaktuje się z niczym poza rozmówcą.

## Bezpieczeństwo

Co jest:

- tożsamość to para kluczy Ed25519 generowana na urządzeniu;
- połączenie jest szyfrowane end-to-end (TLS 1.3 w QUIC), obie strony dowodzą swojego klucza, także gdy pakiety idą przez przekaźnik;
- połączenia przyjmowane są tylko od osób z listy kontaktów;
- limity rozmiaru ramek i wiadomości, `unsafe` zabroniony w całym projekcie.

Czego jeszcze nie ma:

- Double Ratchet nad transportem (ochrona wiadomości czekających w kolejce i zapisanych);
- szyfrowania danych na dysku: klucz leży w pliku `identity.key` z prawami tylko dla właściciela;
- hybrydowej wymiany kluczy odpornej na komputery kwantowe;
- ukrycia adresu IP przed kontaktami;
- niezależnego audytu.

## Budowanie

Wymagany Rust z `rust-toolchain.toml` (rustup pobierze go sam).

```
cargo test --workspace
cargo build --release -p komunikator-cli    # target/release/komunikator
```

GitHub Actions (`.github/workflows/ci.yml`) przy każdym pushu sprawdza format, clippy i testy oraz buduje binaria dla Linuksa, Windows i macOS. Tag `v*` publikuje je jako release z sumami SHA-256. Akcje są przypięte do konkretnych commitów, a Dependabot zgłasza ich aktualizacje.

## Układ repozytorium

```
crates/core   biblioteka: tożsamość, kontakty, protokół, sieć
crates/cli    klient terminalowy
```

Interfejs graficzny (Flutter) będzie korzystał z tej samej biblioteki `core`.

## Plan

1. ~~Rdzeń i czat 1:1 w terminalu~~
2. Double Ratchet, trwała kolejka, zaszyfrowana baza (SQLCipher), klucz w magazynie systemowym
3. Aplikacja Flutter: desktop, potem Android
4. Załączniki i lokalny sejf na materiały
5. Backend Proton Drive, grupy, wiele urządzeń

## Licencja

GPL-3.0-or-later, pełny tekst w pliku `LICENSE`.

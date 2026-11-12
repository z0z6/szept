# Szept

Szyfrowany komunikator tekstowy P2P: bez kont, bez numeru telefonu i bez serwera przechowującego wiadomości.

**Stan: kamień milowy 1 plus wczesne aplikacje okienkowe.** Działa rdzeń, klient terminalowy oraz aplikacje do rozmów 1:1 na Androida, Linuksa, Windows i macOS. To wersja deweloperska, jeszcze nie do poufnej komunikacji (patrz „Bezpieczeństwo”).

## Szybki start

Na obu komputerach:

```
szept init                  # tworzy tożsamość i wypisuje identyfikator
szept add ola <ID_OLI>      # identyfikator przekaż zaufanym kanałem
szept chat ola
```

Wiadomość napisana, gdy kontakt jest offline, czeka w kolejce i wychodzi po jego powrocie. Kolejka żyje na razie tylko do zamknięcia programu.

### Tryb lokalny, bez żadnych zewnętrznych serwerów

```
szept chat ola --local                           # wypisze adresy IP:PORT
szept chat ala --local --direct 192.168.1.20:PORT
```

Opcję `--direct` podaje jedna strona. Druga czeka na połączenie.

W rozmowie `/plik <ścieżka>` wysyła plik, a `/pobierz <numer>` pobiera ofertę do bieżącego katalogu.

## Pliki

Szept przesyła pliki dowolnego typu, także programy (`.exe`, `.apk`, `.deb`, `.AppImage`), bezpośrednio między urządzeniami, bez żadnego serwera z plikami:

- nadawca wysyła ofertę: nazwę, rozmiar i skrót BLAKE3; plik zostaje na jego urządzeniu;
- odbiorca pobiera go prosto od nadawcy tym samym szyfrowanym połączeniem; przerwane pobieranie wznawia się od miejsca przerwania;
- po pobraniu skrót jest sprawdzany i plik niezgodny z ofertą jest usuwany, więc podmiana po drodze albo u nadawcy nie przejdzie;
- pobrać plik może tylko kontakt, któremu go zaoferowano;
- nazwy z sieci, które mogłyby wyjść poza folder pobranych (`../`, nazwy urządzeń Windows), są odrzucane, a istniejący plik nigdy nie jest nadpisywany.

Nadawca musi mieć Szept włączony, dopóki odbiorca nie pobierze pliku. Oferty żyją do zamknięcia aplikacji, a na Androidzie wysyłany plik jest najpierw kopiowany do prywatnej pamięci aplikacji.

Programy nigdy nie otwierają się same. Przy programie od kontaktu widać ostrzeżenie i początek skrótu („odcisk”), który można porównać z nadawcą innym kanałem. Na komputerze program uruchamia się z folderu, a na Androidzie przycisk „Zainstaluj” przekazuje APK systemowemu instalatorowi, który i tak pyta o zgodę. Zdjęcia mają podgląd, a muzykę i nagrania można odtworzyć w rozmowie (na komputerze).

Pobrane pliki trafiają do `Pobrane/Szept` na komputerze i do prywatnej pamięci aplikacji na Androidzie, skąd „Zapisz jako…” kopiuje je w wybrane miejsce.

## Wiadomości głosowe, GIF-y i zdjęcie profilowe

- Gdy pole wiadomości jest puste, w miejscu przycisku „Wyślij” jest mikrofon. Nagranie wysyła „Wyślij”, a „Anuluj” (albo Esc, albo „Wstecz”) je porzuca. Najdłuższe nagranie trwa 10 minut. Android nagrywa AAC (`.m4a`); komputer to, co umie jego silnik przeglądarki (Opus w `.webm`/`.ogg` albo AAC).
- GIF-y i naklejki wstawia się z klawiatury systemowej: na Androidzie z panelu GIF-ów Gboard, SwiftKey czy klawiatury Samsunga, na komputerze przez wklejenie obrazu do pola wiadomości (także z zakładki GIF w panelu Win + . na Windows).
- Wiadomości głosowe i GIF-y są zwykłymi plikami P2P z limitem 16 MB, które aplikacja odbiorcy pobiera sama, bez naciskania „Pobierz”. Jak każdy plik czekają u nadawcy, więc nadawca musi mieć Szept włączony, dopóki nie dotrą.
- Zdjęcie profilowe ustawia się w ustawieniach (zdjęcie w prawym górnym rogu). Jest przycinane do kwadratu i zmniejszane do JPEG poniżej 48 kB. Kontakty dostają je przy każdym połączeniu, więc zmiana dotrze także do tych, którzy są teraz offline. Zdjęcia kontaktów zostają po restarcie.
- Dziesięć motywów kolorystycznych (Szept, Las, Morze, Wrzos, Bursztyn, Malina, Grafit, Koral, Lazur, Oliwka), każdy w wersji jasnej i ciemnej, z wyborem „Jak w systemie / Jasny / Ciemny”. Wybór zostaje na urządzeniu.
- Starsze wersje Szeptu pomijają głosówki, GIF-y i zdjęcia profilowe, a zwykłe wiadomości i pliki działają z nimi jak dotąd.

## Wspólne dla aplikacji okienkowych

- Każdy ma swój kod QR z identyfikatorem (`szept:<identyfikator>`). Kontakt dodaje się, skanując kod drugiej osoby aparatem lub kamerą internetową, wczytując obraz z kodem (na komputerze) albo wklejając identyfikator.
- Dodanie działa w jedną stronę: druga osoba też musi dodać Ciebie, bo Szept przyjmuje połączenia tylko od zapisanych kontaktów.
- Każdy kontakt ma osobną rozmowę. Twoje wiadomości mają status: w kolejce, wysłano (✓) i dostarczono (✓✓).
- Nowa wiadomość w rozmowie, której nie masz przed oczami, daje powiadomienie systemowe i licznik nieprzeczytanych. Powiadomienie pokazuje nadawcę, nigdy treść.
- Emoji to zwykłe znaki Unicode rysowane systemową czcionką.

Wiadomości i pliki przychodzą także w tle, przez Wi‑Fi i dane mobilne: na komputerze, dopóki okno Szeptu jest otwarte (choćby zminimalizowane), na Androidzie, dopóki aplikacja nie zostanie wyłączona przyciskiem „Wyłącz” w jej powiadomieniu albo zatrzymana w ustawieniach systemu. Po zmianie sieci Szept od razu łączy się ponownie. Historia rozmów żyje w pamięci do zamknięcia aplikacji.

## Android

Aplikacja (`android/`, Kotlin i Jetpack Compose) korzysta z rdzenia w Ruście przez wygenerowane wiązania UniFFI. Skaner kodów to ZXing (Apache 2.0, bez usług Google), wybór emoji to systemowy `EmojiPickerView` z AndroidX. Aparat jest opcjonalny: bez niego identyfikator się wkleja.

W tle działa usługa pierwszoplanowa typu „remote messaging” (przewidziana w Androidzie dla komunikatorów), widoczna jako ciche, stałe powiadomienie z przyciskiem „Wyłącz”. Zsunięcie aplikacji z listy ostatnich jej nie zatrzymuje. Szept nie korzysta z Google FCM, więc na telefonach, które agresywnie oszczędzają baterię, warto pozwolić mu działać bez optymalizacji (aplikacja sama o to zapyta). Utrzymywanie połączenia zużywa trochę baterii.

Ograniczenia tej wersji:

- APK jest podpisany kluczem debug, innym przy każdym buildzie w CI, więc przed instalacją nowszej wersji trzeba odinstalować starą (razem z tożsamością i kontaktami).

Build lokalny: `bash scripts/build-android.sh` (wymaga Android SDK i NDK, JDK 17 lub 21 oraz `cargo install cargo-ndk`). APK trafia do `android/app/build/outputs/apk/debug/`. Architektury: arm64-v8a (telefony) i x86_64 (emulator).

## Linux, Windows i macOS

Aplikacja (`crates/desktop`, [Tauri 2](https://tauri.app)) ma okno z kontaktami i osobne okno dla każdej rozmowy. Interfejs to strony HTML w systemowym silniku przeglądarki, więc emoji wyglądają jak w reszcie systemu, a na Windows (Win + .) i macOS (Ctrl + Cmd + Spacja) działa systemowy wybór emoji. Kody QR odczytuje [jsQR](https://github.com/cozmo/jsQR) (Apache 2.0) lokalnie, bez wysyłania obrazu dokądkolwiek.

Paczki z CI:

- Linux: AppImage i .deb, dla dystrybucji z glibc 2.35 i nowszą (Ubuntu 22.04+, Debian 12+, Fedora 36+). Kamera i dźwięk wymagają wtyczek GStreamera; AppImage ma je w środku, a .deb doinstaluje `gstreamer1.0-plugins-good` i `gstreamer1.0-libav` (to drugie odtwarza głosówki z Androida, nagrane w AAC).
- Windows 10 i 11: instalator `-setup.exe`. Korzysta z WebView2, który jest w systemie.
- macOS 12+ na procesorach Apple: `.dmg`, podpisany tylko lokalnie (ad hoc). Przy pierwszym uruchomieniu kliknij aplikację z wciśniętym Ctrl i wybierz „Otwórz”.

Aplikacja okienkowa ma własną tożsamość, osobną od klienta terminalowego.

Build lokalny: w `crates/desktop` uruchom `npm ci`, a potem `npx tauri build`. Na Linuksie potrzebne są `libwebkit2gtk-4.1-dev`, `libxdo-dev` i `librsvg2-dev`.

## Sieć i „brak serwera”

- Urządzenia łączą się bezpośrednio przez QUIC ([iroh](https://github.com/n0-computer/iroh)), adresowane kluczem publicznym.
- W trybie domyślnym pomagają dwa rodzaje publicznych serwerów projektu iroh: wyszukiwanie adresu po kluczu oraz przekaźnik (relay) dla sieci, których NAT nie da się przebić. Widzą, kto z kim się łączy, nie widzą treści. Oba można zastąpić własnymi.
- W trybie `--local` program nie kontaktuje się z niczym poza rozmówcą.

## Bezpieczeństwo

Co jest:

- tożsamość to para kluczy Ed25519 generowana na urządzeniu;
- połączenie jest szyfrowane end-to-end (TLS 1.3 w QUIC, rustls z aws-lc-rs), obie strony dowodzą swojego klucza Ed25519, także gdy pakiety idą przez przekaźnik;
- wymiana kluczy jest hybrydowa i odporna na komputery kwantowe: X25519MLKEM768, czyli X25519 połączone z ML-KEM-768 (NIST FIPS 203). Nagrany dziś ruch pozostaje tajny, dopóki nie zostaną złamane oba algorytmy. Dwa urządzenia z Szeptem zawsze uzgadniają hybrydę, a podpisane uzgadnianie TLS 1.3 nie pozwala atakującemu zepchnąć ich do samego X25519. Klasyczne grupy zostają tylko dla serwerów pomocniczych iroh i starszych wersji Szeptu. Test w `crates/core/src/crypto.rs` sprawdza to na prawdziwym połączeniu z węzłem, który przyjmuje wyłącznie hybrydę;
- dane w połączeniu chroni AES-GCM albo ChaCha20-Poly1305, z nowymi kluczami dla każdego połączenia (forward secrecy);
- połączenia przyjmowane są tylko od osób z listy kontaktów;
- pliki idą bezpośrednio między urządzeniami i są sprawdzane skrótem BLAKE3;
- historia rozmów nie jest nigdzie zapisywana: znika po zamknięciu aplikacji;
- limity rozmiaru ramek i wiadomości;
- `unsafe` zabroniony w rdzeniu i kliencie terminalowym (most `crates/ffi` zawiera go tylko w kodzie generowanym przez UniFFI).

Czego jeszcze nie ma:

- Double Ratchet nad transportem (ochrona wiadomości czekających w kolejce i zapisanych);
- szyfrowania danych na dysku: klucz leży w pliku `identity.key` z prawami tylko dla właściciela, a pobrane pliki leżą w zwykłym folderze. Przed kimś, kto dostanie Twój komputer lub telefon do ręki, chroni dziś tylko szyfrowanie całego dysku (LUKS, BitLocker, FileVault, szyfrowanie Androida) i blokada ekranu;
- podpisów odpornych na komputery kwantowe: tożsamość to nadal Ed25519. To zagraża tylko podszywaniu się w przyszłości, nie poufności nagranego dziś ruchu;
- ukrycia adresu IP przed kontaktami; publiczne serwery pomocnicze iroh widzą, kto z kim się łączy (tryb `--local` albo własny przekaźnik tego unika);
- niezależnego audytu.

## Budowanie

Wymagany Rust z `rust-toolchain.toml` (rustup pobierze go sam).

```
cargo test --workspace
cargo build --release -p komunikator-cli    # target/release/szept
```

GitHub Actions (`.github/workflows/ci.yml`) przy każdym pushu sprawdza format, clippy i testy oraz buduje klienta terminalowego dla Linuksa, Windows i macOS, aplikację okienkową dla tych samych systemów i APK dla Androida. Tag `v*` publikuje je jako release z sumami SHA-256. Akcje są przypięte do konkretnych commitów, a Dependabot zgłasza ich aktualizacje.

## Układ repozytorium

```
crates/core     biblioteka: tożsamość, kontakty, protokół, sieć
crates/cli      klient terminalowy
crates/ffi      most dla aplikacji graficznych (UniFFI): kolejka, ponawianie połączeń, kody QR
crates/desktop  aplikacja okienkowa na Linuksa, Windows i macOS (Tauri)
android         aplikacja na Androida
scripts         skrypt budujący APK
```

## Plan

1. ~~Rdzeń i czat 1:1 w terminalu~~
2. Double Ratchet, trwała kolejka, zaszyfrowana baza (SQLCipher), klucz w magazynie systemowym
3. ~~Kody QR, powiadomienia, aplikacja okienkowa na desktop~~; Android: usługa w tle, podpisywanie własnym kluczem
4. Załączniki i lokalny sejf na materiały
5. Backend Proton Drive, grupy, wiele urządzeń

## Licencja

GPL-3.0-or-later, pełny tekst w pliku `LICENSE`.

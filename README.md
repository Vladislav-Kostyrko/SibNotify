# SibNotify

Вертикальная панель уведомлений для рабочего стола: задачи и чаты Битрикс24, Mattermost, почта, календарь, погода и системные показатели — всё в одной выезжающей панели у края экрана.

Tauri 2 (Rust + один `dist/index.html` без сборки фронта).

## Что умеет

- **Блоки-конструктор**: Битрикс24 (задачи / комментарии / чаты), Mattermost, Telegram (бот), почта по IMAP, календарь (Б24 + внешние `.ics`), погода (Гисметео), система (CPU/RAM/диск/сеть/процессы), часы, а также блоки-переходы: WhatsApp, ВКонтакте, Slack, Discord, Viber, Tilda, 1С, amoCRM.
- **Геометрия**: панель не на весь экран (высота в %), прижата к краю, таскается за шапку; отдельная полоска-триггер, её тоже можно тащить и отключать. Выбор монитора.
- **Настройка вида**: темы (тёмная / полночь / графит / светлая), размер шрифта, радиусы, прозрачность, размытие, акцент.
- **Задачи Б24, комментарии и чаты** — через вебхук Битрикс24 (без OAuth-приложения).
- **Mattermost без админки**: вход в окне панели (SSO), токен сессии забирается из cookie.
- **Тост-уведомления** о новых задачах и упоминаниях, **иконка в трее** с меню, **глобальный хоткей** (`Ctrl+Alt+N`), **автозапуск** при входе в систему.

## Скачать

Готовые бинарники собираются GitHub Actions: вкладка **Actions** → последний успешный workflow → артефакты `SibNotify-windows-x64`, `SibNotify-linux-x64`, `SibNotify-macos-arm64`.

## Сборка из исходников

Нужен Rust (stable) и зависимости Tauri 2.

### Windows
```powershell
cargo build --release --manifest-path src-tauri/Cargo.toml
# готовый файл: src-tauri\target\release\sibnotify.exe
```

### Linux (Debian/Ubuntu)
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
                 librsvg2-dev patchelf libssl-dev build-essential pkg-config
cargo build --release --manifest-path src-tauri/Cargo.toml
# готовый файл: src-tauri/target/release/sibnotify
```

### macOS
```bash
xcode-select --install
cargo build --release --manifest-path src-tauri/Cargo.toml
# готовый файл: src-tauri/target/release/sibnotify
```
Для глобального хоткея и уведомлений macOS попросит разрешения в «Системных настройках → Конфиденциальность».

## Настройка

Всё в панели: `⚙` — панель, экран, визуал; `+ Добавить блок` — конструктор блоков; `⚙` на блоке — настройки источника.

- **Битрикс24**: Приложения → Вебхуки, права `task`, `im`, `calendar`, `user`; URL вида `https://<портал>/rest/<id>/<token>/`.
- **Mattermost**: адрес сервера, затем «Войти в Mattermost» → вход через SSO → «Забрать токен».
- **Почта**: провайдер из списка, адрес и пароль приложения (не основной пароль).
- **Календарь**: ссылка на `.ics` (Яндекс / Outlook / iCloud / Google) и/или события Б24, период — «только сегодня» по умолчанию.
- **Погода**: город из списка или свои координаты.

Конфиг лежит в системной папке настроек приложения:
- Windows: `%APPDATA%\ru.sibtrans.notify\config.json`
- Linux: `~/.config/ru.sibtrans.notify/config.json`
- macOS: `~/Library/Application Support/ru.sibtrans.notify/config.json`

## Лицензия

MIT — см. [LICENSE](LICENSE).

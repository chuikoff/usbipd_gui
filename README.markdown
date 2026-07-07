# USBIPD Manager

USBIPD Manager — графическое приложение на Rust с WinAPI для управления USB-устройствами через [usbipd-win](https://github.com/dorssel/usbipd-win) на Windows. Позволяет привязывать (bind), отвязывать (unbind), подключать (attach) и отключать (detach) USB-устройства к WSL, а также настраивать автоматическое подключение (Auto Attach).

## Особенности

- Просмотр списка подключённых USB-устройств.
- Привязка и отвязка устройств с правами администратора.
- Подключение и отключение устройств к WSL.
- Автоматическое подключение с сохранением настроек между запусками.
- Настраиваемый WSL-дистрибутив через `config.json`.
- Интуитивный графический интерфейс.

## Требования

- Windows 10 или новее.
- Установленный `usbipd` (Microsoft Store или [GitHub](https://github.com/dorssel/usbipd-win)).
- WSL 2.
- Rust и Cargo — только для сборки из исходников.

## Установка

### Готовый бинарник

Скачайте `usbipd_gui.exe` из [релизов](https://github.com/chuikoff/usbipd_gui/releases).

### Сборка из исходников

```bash
git clone https://github.com/chuikoff/usbipd_gui.git
cd usbipd_gui
cargo run --release
```

Для операций bind/unbind может потребоваться подтверждение UAC.

## Настройка

Скопируйте `config.example.json` в `config.json` и укажите ваш WSL-дистрибутив:

```json
{
  "auto_attach_devices": [],
  "wsl_distro": "Ubuntu-24.04"
}
```

Имя дистрибутива можно посмотреть командой `wsl -l -v`. Если `config.json` отсутствует, приложение попытается определить дистрибутив автоматически.

## Использование

- **Bind** — привязать выбранное устройство для USB/IP.
- **Unbind** — отвязать устройство.
- **Attach** — подключить устройство к WSL.
- **Detach** — отключить устройство от WSL.
- **Auto Attach** — включить автоматическое подключение (сохраняется в config).
- **Stop Auto-Attach** — остановить автоматическое подключение.
- **Обновить** — обновить список устройств.

## Примечания

- USBdk или активный VPN могут мешать работе `usbipd` — отключите их при проблемах.
- Для bind/unbind требуются права администратора (UAC).
- Auto-Attach восстанавливается при следующем запуске приложения.

## Лицензия

[MIT License](LICENSE).

## Автор

[chuikoff](https://github.com/chuikoff)
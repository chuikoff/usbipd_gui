# USBIPD Manager

USBIPD Manager — это графическое приложение на Rust с использованием WinAPI, которое упрощает управление USB-устройствами через протокол USB/IP на Windows. Оно позволяет привязывать (bind), отвязывать (unbind), подключать (attach) и отключать (detach) USB-устройства к WSL (Windows Subsystem for Linux), а также поддерживает автоматическое подключение (Auto Attach).

## Особенности
- Просмотр списка подключённых USB-устройств.
- Привязка и отвязка устройств с правами администратора.
- Подключение и отключение устройств к WSL.
- Автоматическое подключение устройств с сохранением настроек.
- Интуитивно понятный графический интерфейс с кнопками и списком устройств.

## Требования
- Windows 10 или новее.
- Установленный `usbipd` (доступен через Microsoft Store или GitHub).
- WSL 2.
- Rust и Cargo для сборки.

## Установка
1. Склонируйте репозиторий:
   ```
   git clone https://github.com/ваш_пользователь/usbipd_gui.git
   cd usbipd_gui
   ```
2. Установите зависимости:
   - Убедитесь, что `usbipd` установлен.
   - Установите Rust: [rustup.rs](https://rustup.rs/).
3. Соберите и запустите:
   ```
   cargo run --release
   ```
   (Запуск от имени администратора может потребоваться для некоторых операций.)

## Использование
- **Bind**: Привязывает выбранное устройство для использования с USB/IP.
- **Unbind**: Отвязывает устройство.
- **Attach**: Подключает устройство к WSL.
- **Detach**: Отключает устройство от WSL.
- **Auto Attach**: Включает автоматическое подключение устройства.
- **Stop Auto-Attach**: Останавливает автоматическое подключение.
- **Обновить**: Обновляет список устройств.

## Примечания
- Наличие USBdk или активного VPN-соединения может повлиять на работу `usbipd`. Рекомендуется отключить их при возникновении проблем.
- Требуется запуск с правами администратора для операций bind/unbind.

## Лицензия
[MIT License](LICENSE).

## Сотрудничество
Добро пожаловать к участию! Открывайте issues или отправляйте pull requests.

## Автор
- Разработано chuikoff(https://github.com/chuikoff).

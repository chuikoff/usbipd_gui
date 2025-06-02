use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HMENU, HWND, HFONT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processthreadsapi::ExitProcess;
use winapi::um::shellapi::ShellExecuteW;
use winapi::um::wingdi::{GetStockObject, DEFAULT_GUI_FONT};
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW, InvalidateRect, LoadCursorW,
    LoadIconW, PostQuitMessage, RegisterClassW, SendMessageW, ShowWindow, TranslateMessage, UpdateWindow,
    SetWindowLongPtrW, GetWindowLongPtrW, BS_DEFPUSHBUTTON, CS_HREDRAW, CS_VREDRAW, IDC_ARROW, LBS_NOTIFY, LBS_HASSTRINGS, LB_ADDSTRING, LB_GETCOUNT,
    LB_GETCURSEL, LB_GETTEXT, LB_RESETCONTENT, MSG, SW_SHOW, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL, MessageBoxW, MB_OK, MB_ICONERROR,
    COLOR_WINDOW, CW_USEDEFAULT, SS_LEFT, WM_SETFONT,
};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::process::{Child, Command};
use std::ptr;
use std::str;
use std::thread;
use std::time::Duration;
use std::fs::File;
use std::io::{Read, Write};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Config {
    auto_attach_devices: Vec<String>,
}

struct AppState {
    auto_attach_processes: HashMap<String, Child>,
    config: Config,
}

impl AppState {
    fn new() -> Self {
        let mut state = AppState {
            auto_attach_processes: HashMap::new(),
            config: load_config(),
        };
        let devices_to_stop: Vec<String> = state.config.auto_attach_devices.clone();
        for bus_id in devices_to_stop {
            state.stop_auto_attach(&bus_id);
        }
        state.config.auto_attach_devices.clear();
        state.save_config();
        state
    }

    fn start_auto_attach(&mut self, bus_id: &str, hwnd: HWND) {
        if self.auto_attach_processes.contains_key(bus_id) {
            println!("Auto-Attach уже запущен для устройства {}", bus_id);
            return;
        }

        let command = format!("usbipd attach --wsl Ubuntu-24.04 --busid {} --auto-attach", bus_id);
        println!("Запуск Auto-Attach для устройства {}: {}", bus_id, command);

        match Command::new("cmd")
            .args(&["/C", &command])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.auto_attach_processes.insert(bus_id.to_string(), child);
                if !self.config.auto_attach_devices.contains(&bus_id.to_string()) {
                    self.config.auto_attach_devices.push(bus_id.to_string());
                    self.save_config();
                }
            }
            Err(e) => {
                println!("Ошибка запуска Auto-Attach для {}: {}", bus_id, e);
                show_error(hwnd, &format!("Ошибка запуска Auto-Attach: {}", e));
            }
        }
    }

    fn stop_auto_attach(&mut self, bus_id: &str) {
        if let Some(mut child) = self.auto_attach_processes.remove(bus_id) {
            let _ = child.kill();
            let _ = child.wait();
            println!("Auto-Attach остановлен для устройства {}", bus_id);
            self.config.auto_attach_devices.retain(|id| id != bus_id);
            self.save_config();
        }
    }

    fn save_config(&self) {
        if let Ok(mut file) = File::create("config.json") {
            if let Ok(json) = serde_json::to_string(&self.config) {
                let _ = file.write_all(json.as_bytes());
            }
        }
    }
}

fn load_config() -> Config {
    if let Ok(mut file) = File::open("config.json") {
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            if let Ok(config) = serde_json::from_str(&contents) {
                return config;
            }
        }
    }
    Config {
        auto_attach_devices: Vec::new(),
    }
}

fn main() {
    unsafe {
        let class_name: Vec<u16> = OsStr::new("USBIPD_GUI").encode_wide().chain(once(0)).collect();
        let h_instance = GetModuleHandleW(ptr::null());
        let h_icon = LoadIconW(h_instance, OsStr::new("icon.ico").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr());
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance,
            hIcon: h_icon,
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as _,
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            ExitProcess(1);
        }

        // Создаём состояние
        let state = Box::new(AppState::new());
        let state_ptr = Box::into_raw(state);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            OsStr::new("USBIPD Manager").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            CW_USEDEFAULT, CW_USEDEFAULT, 800, 700,
            ptr::null_mut(), ptr::null_mut(), h_instance, ptr::null_mut(),
        );
        if hwnd.is_null() {
            // Освобождаем память, если окно не создалось
            let _ = Box::from_raw(state_ptr);
            ExitProcess(1);
        }

        // Сохраняем указатель на состояние в окне
        SetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA, state_ptr as isize);

        let hwnd_list = CreateWindowExW(
            0,
            OsStr::new("LISTBOX").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | LBS_NOTIFY | LBS_HASSTRINGS,
            10, 10, 760, 480,
            hwnd, 100 as HMENU, h_instance, ptr::null_mut(),
        );
        if hwnd_list.is_null() {
            println!("Ошибка создания ListBox");
            ExitProcess(1);
        }

        // Устанавливаем шрифт для ListBox
        let font: HFONT = GetStockObject(DEFAULT_GUI_FONT.try_into().unwrap()) as HFONT;
        SendMessageW(hwnd_list, WM_SETFONT, font as WPARAM, 1 as LPARAM);

        // Статический текст о USBdk и VPN
        let warning_text = OsStr::new("Примечание: USBdk или VPN могут повлиять на работу usbipd.\r\nРекомендуется отключить их при проблемах.")
            .encode_wide()
            .chain(once(0))
            .collect::<Vec<u16>>();
        let hwnd_static = CreateWindowExW(
            0,
            OsStr::new("STATIC").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            warning_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            10, 500, 760, 40, // Увеличиваем высоту для двух строк
            hwnd, 200 as HMENU, h_instance, ptr::null_mut(),
        );
        SendMessageW(hwnd_static, WM_SETFONT, font as WPARAM, 1 as LPARAM);

        // Первый ряд кнопок
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Bind").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            10, 550, 100, 40, hwnd, 101 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Unbind").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            120, 550, 100, 40, hwnd, 102 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Attach").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            230, 550, 100, 40, hwnd, 103 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Detach").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            340, 550, 100, 40, hwnd, 104 as HMENU, h_instance, ptr::null_mut(),
        );

        // Второй ряд кнопок
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Auto Attach").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            10, 600, 130, 40, hwnd, 105 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Stop Auto-Attach").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            150, 600, 150, 40, hwnd, 107 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Обновить").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            310, 600, 100, 40, hwnd, 106 as HMENU, h_instance, ptr::null_mut(),
        );

        // Устанавливаем шрифт для кнопок
        for id in 101..=107 {
            let hwnd_button = GetDlgItem(hwnd, id);
            SendMessageW(hwnd_button, WM_SETFONT, font as WPARAM, 1 as LPARAM);
        }

        populate_usb_list(hwnd_list, hwnd);

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            // Получаем указатель на состояние из окна
            let state_ptr = GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let mut state = Box::from_raw(state_ptr); // Делаем mutable
            let control_id = (wparam & 0xFFFF) as u16;
            match control_id {
                101 => { // Bind
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        let hwnd_list = GetDlgItem(hwnd, 100);
                        let state_str = get_list_item_state(hwnd_list).unwrap_or("Unknown".to_string());
                        if state_str == "Not shared" || state_str == "Unknown" {
                            println!("Попытка выполнить bind для bus_id: {}", bus_id);
                            match run_usbipd_bind_with_admin(&bus_id, hwnd) {
                                Ok(()) => {
                                    println!("Bind успешно выполнен для bus_id: {}", bus_id);
                                    thread::sleep(Duration::from_secs(1));
                                    let hwnd_list = GetDlgItem(hwnd, 100);
                                    populate_usb_list(hwnd_list, hwnd);
                                }
                                Err(err) => {
                                    println!("Ошибка bind для bus_id {}: {}", bus_id, err);
                                    show_error(hwnd, &format!("Не удалось выполнить bind: {}", err));
                                }
                            }
                        } else {
                            show_error(hwnd, "Устройство уже привязано");
                        }
                    } else {
                        show_error(hwnd, "Устройство не выбрано");
                    }
                }
                102 => { // Unbind
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        let hwnd_list = GetDlgItem(hwnd, 100);
                        let state_str = get_list_item_state(hwnd_list).unwrap_or("Unknown".to_string());
                        if state_str == "Shared" || state_str == "Attached" || state_str == "Shared (forced)" {
                            state.stop_auto_attach(&bus_id); // Теперь можно mut
                            match run_usbipd_unbind_with_admin(&bus_id, hwnd) {
                                Ok(()) => {
                                    thread::sleep(Duration::from_secs(1));
                                    let hwnd_list = GetDlgItem(hwnd, 100);
                                    populate_usb_list(hwnd_list, hwnd);
                                }
                                Err(err) => {
                                    println!("Ошибка unbind для bus_id {}: {}", bus_id, err);
                                    show_error(hwnd, &format!("Не удалось выполнить unbind: {}", err));
                                }
                            }
                        } else {
                            show_error(hwnd, "Устройство не привязано или не в подходящем состоянии");
                        }
                    } else {
                        show_error(hwnd, "Устройство не выбрано");
                    }
                }
                103 => { // Attach
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        println!("Выбранное устройство для Attach: bus_id = {}", bus_id);
                        let command = format!("usbipd attach --wsl Ubuntu-24.04 --busid {}", bus_id);
                        println!("Выполняется команда: {}", command);
                        match run_usbipd_command(&command) {
                            Ok(()) => {
                                println!("Attach успешно выполнен");
                                let hwnd_list = GetDlgItem(hwnd, 100);
                                populate_usb_list(hwnd_list, hwnd);
                            }
                            Err(err) => {
                                println!("Ошибка attach: {}", err);
                                show_error(hwnd, &format!("Ошибка подключения: {}", err));
                            }
                        }
                    } else {
                        show_error(hwnd, "Устройство не выбрано");
                    }
                }
                104 => { // Detach
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        println!("Выбранное устройство для Detach: bus_id = {}", bus_id);
                        match run_usbipd_command(&format!("usbipd detach --busid {}", bus_id)) {
                            Ok(()) => {
                                println!("Detach успешно выполнен");
                                let hwnd_list = GetDlgItem(hwnd, 100);
                                populate_usb_list(hwnd_list, hwnd);
                            }
                            Err(err) => {
                                println!("Ошибка detach: {}", err);
                                show_error(hwnd, &format!("Ошибка отключения: {}", err));
                            }
                        }
                    } else {
                        show_error(hwnd, "Устройство не выбрано");
                    }
                }
                105 => { // Auto Attach
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        let hwnd_list = GetDlgItem(hwnd, 100);
                        let state_str = get_list_item_state(hwnd_list).unwrap_or("Unknown".to_string());
                        if state_str == "Shared" {
                            state.start_auto_attach(&bus_id, hwnd);
                            populate_usb_list(hwnd_list, hwnd);
                        } else {
                            show_error(hwnd, "Устройство должно быть в состоянии Shared для Auto-Attach");
                        }
                    } else {
                        show_error(hwnd, "Устройство не выбрано");
                    }
                }
                107 => { // Stop Auto-Attach
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        state.stop_auto_attach(&bus_id);
                        let hwnd_list = GetDlgItem(hwnd, 100);
                        populate_usb_list(hwnd_list, hwnd);
                    } else {
                        show_error(hwnd, "Устройство не выбрано");
                    }
                }
                106 => { // Обновить
                    let hwnd_list = GetDlgItem(hwnd, 100);
                    populate_usb_list(hwnd_list, hwnd);
                }
                _ => {}
            }
            // Игнорируем возвращаемое значение, чтобы избежать предупреждения
            let _ = Box::into_raw(state);
            0
        }
        WM_DESTROY => {
            // Получаем указатель на состояние из окна
            let state_ptr = GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
            if !state_ptr.is_null() {
                let mut state = Box::from_raw(state_ptr); // Делаем mutable
                let devices_to_stop: Vec<String> = state.config.auto_attach_devices.clone();
                for bus_id in devices_to_stop {
                    state.stop_auto_attach(&bus_id);
                }
                // Box автоматически освободит память
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn populate_usb_list(hwnd_list: HWND, hwnd: HWND) {
    unsafe {
        SendMessageW(hwnd_list, LB_RESETCONTENT, 0, 0);
        println!("Очистка списка завершена");

        let output = match Command::new("usbipd").arg("list").output() {
            Ok(output) => output,
            Err(e) => {
                println!("Ошибка выполнения usbipd list: {}", e);
                show_error(hwnd, &format!("Ошибка выполнения usbipd list: {}", e));
                return;
            }
        };

        let output_str = match str::from_utf8(&output.stdout) {
            Ok(s) => s,
            Err(e) => {
                println!("Ошибка декодирования вывода usbipd: {}", e);
                show_error(hwnd, &format!("Ошибка декодирования вывода: {}", e));
                return;
            }
        };
        println!("Вывод usbipd list: {}", output_str);

        let mut lines = output_str.lines().skip(2);
        while let Some(line) = lines.next() {
            if line.is_empty() || line.contains("Persisted:") {
                break;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let bus_id = parts[0];
                let mut device_name = String::new();
                let mut i = 2;
                while i < parts.len() && !parts[i].starts_with("Not") && !parts[i].starts_with("Attached") && !parts[i].starts_with("Shared") {
                    if !device_name.is_empty() {
                        device_name.push(' ');
                    }
                    device_name.push_str(parts[i]);
                    i += 1;
                }
                let state_str = if i < parts.len() { parts[i..].join(" ") } else { "Unknown".to_string() };
                // Получаем указатель на состояние из окна
                let state_ptr = GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
                let state = &*state_ptr;
                let display = if state.config.auto_attach_devices.contains(&bus_id.to_string()) {
                    format!("{}: {} [{}] [Auto-Attach]", bus_id, device_name, state_str)
                } else {
                    format!("{}: {} [{}]", bus_id, device_name, state_str)
                };
                println!("Добавляем строку: {}", display);
                let display_w: Vec<u16> = OsStr::new(&display).encode_wide().chain(once(0)).collect();
                let result = SendMessageW(hwnd_list, LB_ADDSTRING, 0, display_w.as_ptr() as LPARAM);
                if result == -1 {
                    println!("Ошибка добавления строки: {} (hwnd_list: {:p})", display, hwnd_list);
                } else {
                    println!("Строка добавлена: {} (индекс: {})", display, result);
                }
            } else {
                println!("Некорректная строка: {}", line);
            }
        }

        let count = SendMessageW(hwnd_list, LB_GETCOUNT, 0, 0);
        println!("Количество элементов в списке: {}", count);

        if InvalidateRect(hwnd_list, ptr::null(), 1) == 0 {
            println!("Ошибка обновления окна ListBox");
        } else {
            println!("Окно ListBox обновлено");
        }
        UpdateWindow(hwnd_list);
    }
}

fn get_selected_device(hwnd: HWND, list_id: i32) -> Option<String> {
    unsafe {
        let hwnd_list = GetDlgItem(hwnd, list_id);
        if hwnd_list.is_null() {
            println!("Ошибка: hwnd_list is null");
            return None;
        }
        let index = SendMessageW(hwnd_list, LB_GETCURSEL, 0, 0);
        println!("Индекс выбранного элемента: {}", index);
        if index != -1 {
            let mut buffer: [u16; 256] = [0; 256];
            let len = SendMessageW(hwnd_list, LB_GETTEXT, index as WPARAM, buffer.as_mut_ptr() as LPARAM);
            println!("Длина текста: {}", len);
            if len > 0 {
                let text = String::from_utf16_lossy(&buffer[..len as usize]);
                println!("Выбранный текст: {}", text);
                if let Some(bus_id) = text.splitn(2, ": ").next() {
                    return Some(bus_id.to_string());
                }
            }
        }
        None
    }
}

fn get_list_item_state(hwnd_list: HWND) -> Option<String> {
    unsafe {
        let index = SendMessageW(hwnd_list, LB_GETCURSEL, 0, 0);
        if index != -1 {
            let mut buffer: [u16; 256] = [0; 256];
            let len = SendMessageW(hwnd_list, LB_GETTEXT, index as WPARAM, buffer.as_mut_ptr() as LPARAM);
            if len > 0 {
                let text = String::from_utf16_lossy(&buffer[..len as usize]);
                if let Some(state) = text.split('[').nth(1).map(|s| s.trim_end_matches(']').to_string()) {
                    return Some(state.split(']').next().unwrap_or("Unknown").to_string());
                }
            }
        }
        None
    }
}

fn run_usbipd_bind_with_admin(bus_id: &str, hwnd: HWND) -> Result<(), String> {
    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = OsStr::new("cmd.exe").encode_wide().chain(once(0)).collect();
    let params: Vec<u16> = OsStr::new(&format!("/C usbipd bind --busid {} --force", bus_id))
        .encode_wide()
        .chain(once(0))
        .collect();

    println!("Запуск команды: cmd.exe /C usbipd bind --busid {} --force", bus_id);
    let result = unsafe {
        ShellExecuteW(hwnd, verb.as_ptr(), file.as_ptr(), params.as_ptr(), ptr::null(), SW_SHOW)
    };

    if result as i32 > 32 {
        println!("Команда bind выполнена успешно, код: {:?}", result);
        Ok(())
    } else {
        Err(format!("Не удалось запустить usbipd bind с правами администратора (код ошибки: {:?})", result))
    }
}

fn run_usbipd_unbind_with_admin(bus_id: &str, hwnd: HWND) -> Result<(), String> {
    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = OsStr::new("cmd.exe").encode_wide().chain(once(0)).collect();
    let params: Vec<u16> = OsStr::new(&format!("/C usbipd unbind --busid {}", bus_id))
        .encode_wide()
        .chain(once(0))
        .collect();

    println!("Запуск команды: cmd.exe /C usbipd unbind --busid {}", bus_id);
    let result = unsafe {
        ShellExecuteW(hwnd, verb.as_ptr(), file.as_ptr(), params.as_ptr(), ptr::null(), SW_SHOW)
    };

    if result as i32 > 32 {
        println!("Команда unbind выполнена успешно, код: {:?}", result);
        Ok(())
    } else {
        Err(format!("Не удалось запустить usbipd unbind с правами администратора (код ошибки: {:?})", result))
    }
}

fn run_usbipd_command(command: &str) -> Result<(), String> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if let Some((cmd, args)) = parts.split_first() {
        let mut child = Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Не удалось запустить команду: {}", e))?;

        let status = child.wait().map_err(|e| format!("Ошибка ожидания команды: {}", e))?;
        if status.success() {
            let output = child.wait_with_output().map_err(|e| format!("Не удалось получить вывод: {}", e))?;
            println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
            println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
            Ok(())
        } else {
            let output = child.wait_with_output().map_err(|e| format!("Не удалось получить вывод: {}", e))?;
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(format!("Команда завершилась с ошибкой: {}", stderr))
        }
    } else {
        Err("Неверная команда".to_string())
    }
}

fn show_error(hwnd: HWND, message: &str) {
    let title: Vec<u16> = OsStr::new("Ошибка").encode_wide().chain(once(0)).collect();
    let message_w: Vec<u16> = OsStr::new(message).encode_wide().chain(once(0)).collect();
    unsafe {
        MessageBoxW(hwnd, message_w.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}
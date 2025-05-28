use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBRUSH, HMENU, HWND};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processthreadsapi::ExitProcess;
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW, LoadCursorW,
    PostQuitMessage, RegisterClassW, SendMessageW, ShowWindow, TranslateMessage, BS_DEFPUSHBUTTON,
    CS_HREDRAW, CS_VREDRAW, IDC_ARROW, LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, LB_GETTEXT,
    LB_RESETCONTENT, MSG, SW_SHOW, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL,
};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::process::Command;
use std::ptr;
use std::str;

fn main() {
    unsafe {
        // Получение экземпляра модуля
        let h_instance = GetModuleHandleW(ptr::null());

        // Подготовка имени класса окна
        let class_name: Vec<u16> = OsStr::new("USBIPD_GUI")
            .encode_wide()
            .chain(once(0))
            .collect();

        // Регистрация класса окна
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance,
            hIcon: ptr::null_mut(),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: 16 as HBRUSH, // COLOR_WINDOW + 1
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            ExitProcess(1); // Завершаем, если регистрация не удалась
        }

        // Создание главного окна
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            OsStr::new("USBIPD Manager")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            WS_OVERLAPPEDWINDOW,
            -1, // CW_USEDEFAULT
            -1, // CW_USEDEFAULT
            400,
            400,
            ptr::null_mut(),
            ptr::null_mut(),
            h_instance,
            ptr::null_mut(),
        );
        if hwnd.is_null() {
            ExitProcess(1); // Завершаем, если окно не создано
        }

        // Создание ListBox
        let hwnd_list = CreateWindowExW(
            0,
            OsStr::new("LISTBOX")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | LBS_NOTIFY,
            10,
            10,
            360,
            200,
            hwnd,
            100 as HMENU,
            h_instance,
            ptr::null_mut(),
        );

        // Кнопка "Attach"
        CreateWindowExW(
            0,
            OsStr::new("BUTTON")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            OsStr::new("Attach")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            10,
            220,
            100,
            30,
            hwnd,
            101 as HMENU,
            h_instance,
            ptr::null_mut(),
        );

        // Кнопка "Detach"
        CreateWindowExW(
            0,
            OsStr::new("BUTTON")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            OsStr::new("Detach")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
            120,
            220,
            100,
            30,
            hwnd,
            102 as HMENU,
            h_instance,
            ptr::null_mut(),
        );

        // Заполнение списка устройств
        populate_usb_list(hwnd_list);

        // Показ окна
        ShowWindow(hwnd, SW_SHOW);

        // Цикл сообщений
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
            let control_id = (wparam & 0xFFFF) as u16;
            match control_id {
                101 => {
                    // Кнопка "Attach"
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        run_usbipd_command(&format!("usbipd attach --wsl --busid {}", bus_id));
                    }
                }
                102 => {
                    // Кнопка "Detach"
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        run_usbipd_command(&format!("usbipd detach --busid {}", bus_id));
                    }
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn populate_usb_list(hwnd_list: HWND) {
    unsafe {
        // Очистка списка
        SendMessageW(hwnd_list, LB_RESETCONTENT, 0, 0);

        // Выполнение команды usbipd list
        let output = Command::new("usbipd")
            .arg("list")
            .output()
            .expect("Не удалось выполнить usbipd list");

        let output_str = str::from_utf8(&output.stdout).expect("Невалидный UTF-8 вывод");
        println!("Вывод usbipd list: {}", output_str); // Отладочный вывод

        // Парсинг вывода и добавление в список
        for line in output_str.lines() {
            if line.contains(" - ") {
                let parts: Vec<&str> = line.splitn(2, " - ").collect();
                if parts.len() == 2 {
                    let bus_id = parts[0].trim();
                    let device_name = parts[1].trim();
                    let display = format!("{}: {}", bus_id, device_name);
                    let display_w: Vec<u16> = OsStr::new(&display)
                        .encode_wide()
                        .chain(once(0))
                        .collect();
                    let result = SendMessageW(hwnd_list, LB_ADDSTRING, 0, display_w.as_ptr() as LPARAM);
                    if result == -1 {
                        println!("Ошибка добавления строки: {}", display); // Отладка
                    }
                } else {
                    println!("Некорректная строка: {}", line); // Отладка
                }
            }
        }
    }
}

fn get_selected_device(hwnd: HWND, list_id: i32) -> Option<String> {
    unsafe {
        let hwnd_list = GetDlgItem(hwnd, list_id);
        if hwnd_list.is_null() {
            return None;
        }
        let index = SendMessageW(hwnd_list, LB_GETCURSEL, 0, 0);
        if index != -1 {
            let mut buffer: [u16; 256] = [0; 256];
            let len = SendMessageW(
                hwnd_list,
                LB_GETTEXT,
                index as WPARAM,
                buffer.as_mut_ptr() as LPARAM,
            );
            if len > 0 {
                let text = String::from_utf16_lossy(&buffer[..len as usize]);
                if let Some(bus_id) = text.splitn(2, ": ").next() {
                    return Some(bus_id.to_string());
                }
            }
        }
        None
    }
}

fn run_usbipd_command(command: &str) {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if let Some((cmd, args)) = parts.split_first() {
        Command::new(cmd)
            .args(args)
            .spawn()
            .expect("Не удалось выполнить usbipd команду");
    }
}
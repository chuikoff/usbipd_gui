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
        if hwnd_list.is_null() {
            println!("Ошибка создания ListBox");
            ExitProcess(1);
        }

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
        let output = match Command::new("usbipd").arg("list").output() {
            Ok(output) => output,
            Err(e) => {
                println!("Ошибка выполнения usbipd list: {}", e);
                return;
            }
        };

        let output_str = match str::from_utf8(&output.stdout) {
            Ok(s) => s,
            Err(e) => {
                println!("Ошибка декодирования вывода usbipd: {}", e);
                return;
            }
        };
        println!("Вывод usbipd list: {}", output_str); // Отладочный вывод

        // Пропускаем заголовок
        let mut lines = output_str.lines().skip(2); // Пропускаем "Connected:" и заголовок таблицы
        while let Some(line) = lines.next() {
            if line.is_empty() || line.contains("Persisted:") {
                break; // Прерываем, если дошли до секции Persisted
            }
            // Разделяем строку по пробелам/табам, убираем лишние пробелы
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let bus_id = parts[0]; // BUSID, например "1-6"
                // Собираем DEVICE до слова "Not" или конца строки
                let mut device_name = String::new();
                let mut i = 2; // Начинаем с DEVICE (после BUSID и VID:PID)
                while i < parts.len() && parts[i] != "Not" {
                    if !device_name.is_empty() {
                        device_name.push(' ');
                    }
                    device_name.push_str(parts[i]);
                    i += 1;
                }
                let display = format!("{}: {}", bus_id, device_name);
                let display_w: Vec<u16> = OsStr::new(&display)
                    .encode_wide()
                    .chain(once(0))
                    .
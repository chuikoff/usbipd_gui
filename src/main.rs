use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HBRUSH, HMENU, HWND, RECT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processthreadsapi::ExitProcess;
use winapi::um::shellapi::ShellExecuteW;
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW, InvalidateRect, LoadCursorW,
    PostQuitMessage, RegisterClassW, SendMessageW, ShowWindow, TranslateMessage, UpdateWindow, BS_DEFPUSHBUTTON,
    CS_HREDRAW, CS_VREDRAW, IDC_ARROW, LBS_NOTIFY, LB_ADDSTRING, LB_GETCOUNT, LB_GETCURSEL,
    LB_GETTEXT, LB_RESETCONTENT, MSG, SW_SHOW, WM_COMMAND, WM_DESTROY, WNDCLASSW, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL, MessageBoxW, MB_OK, MB_ICONERROR,
};
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::process::Command;
use std::ptr;
use std::str;
use std::thread;
use std::time::Duration;
use wait_timeout::ChildExt;

fn main() {
    unsafe {
        let h_instance = GetModuleHandleW(ptr::null());
        let class_name: Vec<u16> = OsStr::new("USBIPD_GUI")
            .encode_wide()
            .chain(once(0))
            .collect();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: h_instance,
            hIcon: ptr::null_mut(),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: 16 as HBRUSH,
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        if RegisterClassW(&wc) == 0 {
            ExitProcess(1);
        }

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            OsStr::new("USBIPD Manager")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            WS_OVERLAPPEDWINDOW,
            -1, -1, 600, 600,
            ptr::null_mut(), ptr::null_mut(), h_instance, ptr::null_mut(),
        );
        if hwnd.is_null() {
            ExitProcess(1);
        }

        let hwnd_list = CreateWindowExW(
            0,
            OsStr::new("LISTBOX")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | LBS_NOTIFY,
            10, 10, 560, 500,
            hwnd, 100 as HMENU, h_instance, ptr::null_mut(),
        );
        if hwnd_list.is_null() {
            println!("Ошибка создания ListBox");
            ExitProcess(1);
        }
        let count = SendMessageW(hwnd_list, LB_GETCOUNT, 0, 0);
        println!("После создания ListBox, количество элементов: {}", count);
        println!("hwnd_list: {:p}", hwnd_list);

        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Bind").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON, 10, 520, 80, 30, hwnd, 101 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Unbind").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON, 100, 520, 80, 30, hwnd, 102 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Attach").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON, 190, 520, 80, 30, hwnd, 103 as HMENU, h_instance, ptr::null_mut(),
        );
        CreateWindowExW(
            0, OsStr::new("BUTTON").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            OsStr::new("Detach").encode_wide().chain(once(0)).collect::<Vec<u16>>().as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON, 280, 520, 80, 30, hwnd, 104 as HMENU, h_instance, ptr::null_mut(),
        );

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
            let control_id = (wparam & 0xFFFF) as u16;
            match control_id {
                101 => {
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        let state = check_device_state(&bus_id).unwrap_or("Unknown".to_string());
                        if state == "Not shared" || state == "Unknown" {
                            let bind_result = run_usbipd_bind_with_admin(&bus_id, hwnd);
                            if bind_result.is_ok() {
                                thread::sleep(Duration::from_secs(1));
                                let hwnd_list = GetDlgItem(hwnd, 100);
                                populate_usb_list(hwnd_list, hwnd);
                            } else {
                                show_error(hwnd, &format!("Не удалось выполнить bind: {}", bind_result.unwrap_err()));
                            }
                        } else {
                            show_error(hwnd, "Устройство уже привязано");
                        }
                    }
                }
                102 => {
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        let state = check_device_state(&bus_id).unwrap_or("Unknown".to_string());
                        if state == "Shared" || state == "Attached" {
                            let unbind_result = run_usbipd_unbind_with_admin(&bus_id, hwnd);
                            if unbind_result.is_ok() {
                                thread::sleep(Duration::from_secs(1));
                                let hwnd_list = GetDlgItem(hwnd, 100);
                                populate_usb_list(hwnd_list, hwnd);
                            } else {
                                show_error(hwnd, &format!("Не удалось выполнить unbind: {}", unbind_result.unwrap_err()));
                            }
                        } else {
                            show_error(hwnd, "Устройство не привязано");
                        }
                    }
                }
                103 => {
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        let state = check_device_state(&bus_id).unwrap_or("Unknown".to_string());
                        if state == "Shared" {
                            println!("Попытка выполнить attach для bus_id: {}", bus_id);
                            match run_usbipd_command_with_timeout(&format!("usbipd attach --wsl --busid {}", bus_id), Duration::from_secs(20)) {
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
                            show_error(hwnd, "Устройство не привязано или уже подключено");
                        }
                    }
                }
                104 => {
                    let selected = get_selected_device(hwnd, 100);
                    if let Some(bus_id) = selected {
                        match run_usbipd_command_with_timeout(&format!("usbipd detach --busid {}", bus_id), Duration::from_secs(20)) {
                            Ok(()) => {
                                let hwnd_list = GetDlgItem(hwnd, 100);
                                populate_usb_list(hwnd_list, hwnd);
                            }
                            Err(err) => show_error(hwnd, &format!("Ошибка отключения: {}", err)),
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
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
                let state = if i < parts.len() { parts[i..].join(" ") } else { "Unknown".to_string() };
                let display = format!("{}: {} [{}]", bus_id, device_name, state);
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

        let mut rect: RECT = std::mem::zeroed();
        if InvalidateRect(hwnd_list, &mut rect, 1) == 0 {
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
            return None;
        }
        let index = SendMessageW(hwnd_list, LB_GETCURSEL, 0, 0);
        if index != -1 {
            let mut buffer: [u16; 256] = [0; 256];
            let len = SendMessageW(hwnd_list, LB_GETTEXT, index as WPARAM, buffer.as_mut_ptr() as LPARAM);
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

fn check_device_state(bus_id: &str) -> Option<String> {
    let output = Command::new("usbipd").arg("list").output().ok()?;
    let output_str = str::from_utf8(&output.stdout).ok()?;
    
    let mut lines = output_str.lines().skip(2);
    while let Some(line) = lines.next() {
        if line.is_empty() || line.contains("Persisted:") {
            break;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 && parts[0] == bus_id {
            let mut i = 2;
            while i < parts.len() && !parts[i].starts_with("Not") && !parts[i].starts_with("Attached") && !parts[i].starts_with("Shared") {
                i += 1;
            }
            if i < parts.len() {
                return Some(parts[i..].join(" "));
            }
        }
    }
    None
}

fn run_usbipd_bind_with_admin(bus_id: &str, hwnd: HWND) -> Result<(), String> {
    let verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(once(0)).collect();
    let file: Vec<u16> = OsStr::new("cmd.exe").encode_wide().chain(once(0)).collect();
    let params: Vec<u16> = OsStr::new(&format!("/C usbipd bind --busid {}", bus_id))
        .encode_wide()
        .chain(once(0))
        .collect();

    let result = unsafe {
        ShellExecuteW(hwnd, verb.as_ptr(), file.as_ptr(), params.as_ptr(), ptr::null(), SW_SHOW)
    };

    if result as i32 > 32 {
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

    let result = unsafe {
        ShellExecuteW(hwnd, verb.as_ptr(), file.as_ptr(), params.as_ptr(), ptr::null(), SW_SHOW)
    };

    if result as i32 > 32 {
        Ok(())
    } else {
        Err(format!("Не удалось запустить usbipd unbind с правами администратора (код ошибки: {:?})", result))
    }
}

fn run_usbipd_command_with_timeout(command: &str, timeout: Duration) -> Result<(), String> {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if let Some((cmd, args)) = parts.split_first() {
        let mut child = Command::new(cmd)
            .args(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Не удалось запустить команду: {}", e))?;

        match child.wait_timeout(timeout).map_err(|e| format!("Ошибка ожидания команды: {}", e))? {
            Some(status) if status.success() => {
                let output = child.wait_with_output().map_err(|e| format!("Не удалось получить вывод: {}", e))?;
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                Ok(())
            }
            Some(_) => {
                let output = child.wait_with_output().map_err(|e| format!("Не удалось получить вывод: {}", e))?;
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Err(format!("Команда завершилась с ошибкой: {}", stderr))
            }
            None => {
                child.kill().map_err(|e| format!("Не удалось завершить процесс: {}", e))?;
                child.wait().map_err(|e| format!("Ошибка ожидания завершения процесса: {}", e))?;
                Err("Команда превысила таймаут".to_string())
            }
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
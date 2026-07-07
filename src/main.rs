mod config;
mod usbipd;

use config::{load_config, save_config, Config};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::process::{Child, Command};
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};
use usbipd::{
    attach_auto_command, extract_bus_id, extract_state_from_display, fetch_usb_devices,
    format_device_display, is_auto_attachable_state, is_bindable_state, is_unbindable_state,
    run_usbipd_attach, run_usbipd_bind, run_usbipd_detach, run_usbipd_unbind,
};
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HFONT, HMENU, HWND};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processthreadsapi::ExitProcess;
use winapi::um::wingdi::{GetStockObject, DEFAULT_GUI_FONT};
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDlgItem, GetMessageW, GetWindowLongPtrW,
    InvalidateRect, LoadCursorW, LoadIconW, MessageBoxW, PeekMessageW, PostQuitMessage,
    RegisterClassW, SendMessageW, SetWindowLongPtrW, ShowWindow, TranslateMessage, UpdateWindow,
    BS_DEFPUSHBUTTON, COLOR_WINDOW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW,
    IDI_APPLICATION, LBS_HASSTRINGS, LBS_NOTIFY, LB_ADDSTRING, LB_GETCOUNT, LB_GETCURSEL,
    LB_GETTEXT, LB_RESETCONTENT, MB_ICONERROR, MB_OK, MSG, PM_REMOVE, SS_LEFT, SW_SHOW, WM_COMMAND,
    WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    WS_VSCROLL,
};

struct AppState {
    auto_attach_processes: HashMap<String, Child>,
    config: Config,
}

impl AppState {
    fn new() -> Self {
        Self {
            auto_attach_processes: HashMap::new(),
            config: load_config(),
        }
    }

    fn restore_auto_attach(&mut self, hwnd: HWND) {
        let devices: Vec<String> = self.config.auto_attach_devices.clone();
        for bus_id in devices {
            self.start_auto_attach(&bus_id, hwnd);
        }
    }

    fn start_auto_attach(&mut self, bus_id: &str, hwnd: HWND) {
        if self.auto_attach_processes.contains_key(bus_id) {
            println!("Auto-Attach уже запущен для устройства {bus_id}");
            return;
        }

        let command = attach_auto_command(bus_id, &self.config.wsl_distro);
        println!("Запуск Auto-Attach для устройства {bus_id}: {command}");

        match Command::new("cmd")
            .args(["/C", &command])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.auto_attach_processes.insert(bus_id.to_string(), child);
                if !self
                    .config
                    .auto_attach_devices
                    .contains(&bus_id.to_string())
                {
                    self.config.auto_attach_devices.push(bus_id.to_string());
                    save_config(&self.config);
                }
            }
            Err(e) => {
                println!("Ошибка запуска Auto-Attach для {bus_id}: {e}");
                show_error(hwnd, &format!("Ошибка запуска Auto-Attach: {e}"));
            }
        }
    }

    fn stop_auto_attach(&mut self, bus_id: &str) {
        if let Some(mut child) = self.auto_attach_processes.remove(bus_id) {
            let _ = child.kill();
            let _ = child.wait();
            println!("Auto-Attach остановлен для устройства {bus_id}");
            self.config.auto_attach_devices.retain(|id| id != bus_id);
            save_config(&self.config);
        }
    }

    fn shutdown_auto_attach_processes(&mut self) {
        for (_, mut child) in self.auto_attach_processes.drain() {
            let _ = child.kill();
            let _ = child.wait();
        }
        save_config(&self.config);
    }
}

fn main() {
    unsafe {
        let class_name: Vec<u16> = OsStr::new("USBIPD_GUI")
            .encode_wide()
            .chain(once(0))
            .collect();
        let h_instance = GetModuleHandleW(ptr::null());
        let h_icon = LoadIconW(ptr::null_mut(), IDI_APPLICATION);
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

        let state = Box::new(AppState::new());
        let state_ptr = Box::into_raw(state);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            OsStr::new("USBIPD Manager")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            800,
            700,
            ptr::null_mut(),
            ptr::null_mut(),
            h_instance,
            ptr::null_mut(),
        );
        if hwnd.is_null() {
            let _ = Box::from_raw(state_ptr);
            ExitProcess(1);
        }

        SetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA, state_ptr as isize);

        let hwnd_list = CreateWindowExW(
            0,
            OsStr::new("LISTBOX")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | LBS_NOTIFY | LBS_HASSTRINGS,
            10,
            10,
            760,
            480,
            hwnd,
            100 as HMENU,
            h_instance,
            ptr::null_mut(),
        );
        if hwnd_list.is_null() {
            let _ = Box::from_raw(state_ptr);
            ExitProcess(1);
        }

        let font: HFONT = GetStockObject(DEFAULT_GUI_FONT.try_into().unwrap()) as HFONT;
        SendMessageW(hwnd_list, WM_SETFONT, font as WPARAM, 1 as LPARAM);

        let warning_text = OsStr::new(
            "Примечание: USBdk или VPN могут повлиять на работу usbipd.\r\n\
             Рекомендуется отключить их при проблемах.\r\n\
             WSL-дистрибутив настраивается в config.json (поле wsl_distro).",
        )
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<u16>>();
        let hwnd_static = CreateWindowExW(
            0,
            OsStr::new("STATIC")
                .encode_wide()
                .chain(once(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            warning_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            10,
            500,
            760,
            55,
            hwnd,
            200 as HMENU,
            h_instance,
            ptr::null_mut(),
        );
        SendMessageW(hwnd_static, WM_SETFONT, font as WPARAM, 1 as LPARAM);

        for (label, id, x, y, w, h) in [
            ("Bind", 101, 10, 565, 100, 40),
            ("Unbind", 102, 120, 565, 100, 40),
            ("Attach", 103, 230, 565, 100, 40),
            ("Detach", 104, 340, 565, 100, 40),
            ("Auto Attach", 105, 10, 615, 130, 40),
            ("Stop Auto-Attach", 107, 150, 615, 150, 40),
            ("Обновить", 106, 310, 615, 100, 40),
        ] {
            create_button(hwnd, h_instance, label, id, x, y, w, h);
        }

        for id in 101..=107 {
            let hwnd_button = GetDlgItem(hwnd, id);
            SendMessageW(hwnd_button, WM_SETFONT, font as WPARAM, 1 as LPARAM);
        }

        {
            let state = &mut *state_ptr;
            state.restore_auto_attach(hwnd);
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

#[allow(clippy::too_many_arguments)]
unsafe fn create_button(
    parent: HWND,
    h_instance: winapi::shared::minwindef::HINSTANCE,
    label: &str,
    id: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    CreateWindowExW(
        0,
        OsStr::new("BUTTON")
            .encode_wide()
            .chain(once(0))
            .collect::<Vec<u16>>()
            .as_ptr(),
        OsStr::new(label)
            .encode_wide()
            .chain(once(0))
            .collect::<Vec<u16>>()
            .as_ptr(),
        WS_CHILD | WS_VISIBLE | BS_DEFPUSHBUTTON,
        x,
        y,
        width,
        height,
        parent,
        id as HMENU,
        h_instance,
        ptr::null_mut(),
    );
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let state_ptr =
                GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let mut state = Box::from_raw(state_ptr);
            let control_id = (wparam & 0xFFFF) as u16;
            let hwnd_list = GetDlgItem(hwnd, 100);

            match control_id {
                101 => handle_bind(hwnd, hwnd_list),
                102 => handle_unbind(hwnd, hwnd_list, &mut state),
                103 => handle_attach(hwnd, hwnd_list, &state),
                104 => handle_detach(hwnd, hwnd_list),
                105 => handle_auto_attach(hwnd, hwnd_list, &mut state),
                107 => handle_stop_auto_attach(hwnd, hwnd_list, &mut state),
                106 => populate_usb_list(hwnd_list, hwnd),
                _ => {}
            }

            let _ = Box::into_raw(state);
            0
        }
        WM_DESTROY => {
            let state_ptr =
                GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
            if !state_ptr.is_null() {
                let mut state = Box::from_raw(state_ptr);
                state.shutdown_auto_attach_processes();
            }
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn handle_bind(hwnd: HWND, hwnd_list: HWND) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    let state_str = get_list_item_state(hwnd_list).unwrap_or_else(|| "Unknown".to_string());
    if !is_bindable_state(&state_str) {
        show_error(hwnd, "Устройство уже привязано");
        return;
    }

    println!("Попытка выполнить bind для bus_id: {bus_id}");
    match run_usbipd_bind(&bus_id) {
        Ok(()) => {
            wait_for_device_state(hwnd, &bus_id, |state| !is_bindable_state(state));
            populate_usb_list(hwnd_list, hwnd);
        }
        Err(err) => {
            println!("Ошибка bind для bus_id {bus_id}: {err}");
            show_error(hwnd, &format!("Не удалось выполнить bind: {err}"));
        }
    }
}

fn handle_unbind(hwnd: HWND, hwnd_list: HWND, state: &mut AppState) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    let state_str = get_list_item_state(hwnd_list).unwrap_or_else(|| "Unknown".to_string());
    if !is_unbindable_state(&state_str) {
        show_error(
            hwnd,
            "Устройство не привязано или не в подходящем состоянии",
        );
        return;
    }

    state.stop_auto_attach(&bus_id);
    match run_usbipd_unbind(&bus_id) {
        Ok(()) => {
            wait_for_device_state(hwnd, &bus_id, is_bindable_state);
            populate_usb_list(hwnd_list, hwnd);
        }
        Err(err) => {
            println!("Ошибка unbind для bus_id {bus_id}: {err}");
            show_error(hwnd, &format!("Не удалось выполнить unbind: {err}"));
        }
    }
}

fn handle_attach(hwnd: HWND, hwnd_list: HWND, state: &AppState) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    println!(
        "Attach: bus_id = {bus_id}, wsl = {}",
        state.config.wsl_distro
    );
    match run_usbipd_attach(&bus_id, &state.config.wsl_distro) {
        Ok(()) => populate_usb_list(hwnd_list, hwnd),
        Err(err) => {
            println!("Ошибка attach: {err}");
            show_error(hwnd, &format!("Ошибка подключения: {err}"));
        }
    }
}

fn handle_detach(hwnd: HWND, hwnd_list: HWND) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    match run_usbipd_detach(&bus_id) {
        Ok(()) => populate_usb_list(hwnd_list, hwnd),
        Err(err) => {
            println!("Ошибка detach: {err}");
            show_error(hwnd, &format!("Ошибка отключения: {err}"));
        }
    }
}

fn handle_auto_attach(hwnd: HWND, hwnd_list: HWND, state: &mut AppState) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    let state_str = get_list_item_state(hwnd_list).unwrap_or_else(|| "Unknown".to_string());
    if !is_auto_attachable_state(&state_str) {
        show_error(
            hwnd,
            "Устройство должно быть в состоянии Shared для Auto-Attach",
        );
        return;
    }

    state.start_auto_attach(&bus_id, hwnd);
    populate_usb_list(hwnd_list, hwnd);
}

fn handle_stop_auto_attach(hwnd: HWND, hwnd_list: HWND, state: &mut AppState) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    state.stop_auto_attach(&bus_id);
    populate_usb_list(hwnd_list, hwnd);
}

fn wait_for_device_state(_hwnd: HWND, bus_id: &str, predicate: fn(&str) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(state)) = usbipd::get_device_state(bus_id) {
            if predicate(&state) {
                return;
            }
        }
        pump_pending_messages();
        thread::sleep(Duration::from_millis(200));
    }
}

fn pump_pending_messages() {
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn populate_usb_list(hwnd_list: HWND, hwnd: HWND) {
    unsafe {
        SendMessageW(hwnd_list, LB_RESETCONTENT, 0, 0);

        let auto_attach_devices = {
            let state_ptr =
                GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *const AppState;
            if state_ptr.is_null() {
                Vec::new()
            } else {
                (*state_ptr).config.auto_attach_devices.clone()
            }
        };

        let devices = match fetch_usb_devices() {
            Ok(devices) => devices,
            Err(err) => {
                println!("{err}");
                show_error(hwnd, &err);
                return;
            }
        };

        for device in devices {
            let auto_attach = auto_attach_devices.contains(&device.bus_id);
            let display = format_device_display(&device, auto_attach);
            let display_w: Vec<u16> = OsStr::new(&display).encode_wide().chain(once(0)).collect();
            let result = SendMessageW(hwnd_list, LB_ADDSTRING, 0, display_w.as_ptr() as LPARAM);
            if result == -1 {
                println!("Ошибка добавления строки: {display}");
            }
        }

        let _ = SendMessageW(hwnd_list, LB_GETCOUNT, 0, 0);
        let _ = InvalidateRect(hwnd_list, ptr::null(), 1);
        UpdateWindow(hwnd_list);
    }
}

fn get_selected_device(hwnd_list: HWND) -> Option<String> {
    unsafe {
        if hwnd_list.is_null() {
            return None;
        }
        let index = SendMessageW(hwnd_list, LB_GETCURSEL, 0, 0);
        if index == -1 {
            return None;
        }

        let mut buffer = [0u16; 512];
        let len = SendMessageW(
            hwnd_list,
            LB_GETTEXT,
            index as WPARAM,
            buffer.as_mut_ptr() as LPARAM,
        );
        if len > 0 {
            let text = String::from_utf16_lossy(&buffer[..len as usize]);
            return extract_bus_id(&text);
        }
        None
    }
}

fn get_list_item_state(hwnd_list: HWND) -> Option<String> {
    unsafe {
        let index = SendMessageW(hwnd_list, LB_GETCURSEL, 0, 0);
        if index == -1 {
            return None;
        }

        let mut buffer = [0u16; 512];
        let len = SendMessageW(
            hwnd_list,
            LB_GETTEXT,
            index as WPARAM,
            buffer.as_mut_ptr() as LPARAM,
        );
        if len > 0 {
            let text = String::from_utf16_lossy(&buffer[..len as usize]);
            return extract_state_from_display(&text);
        }
        None
    }
}

fn show_error(hwnd: HWND, message: &str) {
    let title: Vec<u16> = OsStr::new("Ошибка").encode_wide().chain(once(0)).collect();
    let message_w: Vec<u16> = OsStr::new(message).encode_wide().chain(once(0)).collect();
    unsafe {
        MessageBoxW(
            hwnd,
            message_w.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

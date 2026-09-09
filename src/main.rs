mod config;
mod usbipd;

use config::{load_config, save_config, Config};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::iter::once;
use std::mem;
use std::os::windows::io::AsRawHandle;
use std::os::windows::ffi::OsStrExt;
use std::process::Child;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};
use usbipd::{
    extract_bus_id, extract_state_from_display, fetch_usb_devices, format_device_display,
    is_auto_attachable_state, is_bindable_state, is_unbindable_state, run_usbipd_attach,
    run_usbipd_bind, run_usbipd_detach, run_usbipd_unbind, spawn_auto_attach, validate_wsl_distro,
};
use winapi::shared::minwindef::{LPARAM, LRESULT, UINT, WPARAM};
use winapi::shared::windef::{HFONT, HMENU, HWND};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::jobapi2::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::processthreadsapi::ExitProcess;
use winapi::um::wingdi::{GetStockObject, DEFAULT_GUI_FONT};
use winapi::um::winnt::{
    JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetDlgItem,
    GetMessageW, GetWindowLongPtrW, InvalidateRect, LoadCursorW, LoadIconW, MessageBoxW,
    MoveWindow, PostQuitMessage, RegisterClassW, SendMessageW, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, UpdateWindow, BS_DEFPUSHBUTTON, BS_PUSHBUTTON, COLOR_WINDOW, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, IDI_APPLICATION, LBS_HASSTRINGS, LBS_NOTIFY,
    LB_ADDSTRING, LB_GETCURSEL, LB_GETTEXT, LB_RESETCONTENT, MB_ICONERROR, MB_OK, MSG, SS_LEFT,
    SW_SHOW, WM_COMMAND, WM_DESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE, WS_VSCROLL,
};
use winapi::ctypes::c_void;

const ID_LIST: i32 = 100;
const ID_BIND: i32 = 101;
const ID_UNBIND: i32 = 102;
const ID_ATTACH: i32 = 103;
const ID_DETACH: i32 = 104;
const ID_AUTO_ATTACH: i32 = 105;
const ID_REFRESH: i32 = 106;
const ID_STOP_AUTO: i32 = 107;
const ID_STATIC: i32 = 200;

struct AppState {
    auto_attach_processes: HashMap<String, Child>,
    config: Config,
    job: winapi::um::winnt::HANDLE,
}

impl AppState {
    fn new() -> Self {
        let job = unsafe { create_kill_on_close_job() };
        Self {
            auto_attach_processes: HashMap::new(),
            config: load_config(),
            job,
        }
    }

    fn restore_auto_attach(&mut self, hwnd: HWND) {
        if let Err(err) = validate_wsl_distro(&self.config.wsl_distro) {
            show_error(hwnd, &err);
            self.config.auto_attach_devices.clear();
            save_config(&self.config);
            return;
        }

        let devices: Vec<String> = self.config.auto_attach_devices.clone();
        let mut failed = Vec::new();
        for bus_id in devices {
            if self.start_auto_attach(&bus_id, hwnd).is_err() {
                failed.push(bus_id);
            }
        }
        if !failed.is_empty() {
            self.config
                .auto_attach_devices
                .retain(|id| !failed.contains(id));
            save_config(&self.config);
        }
    }

    fn start_auto_attach(&mut self, bus_id: &str, hwnd: HWND) -> Result<(), ()> {
        if self.auto_attach_processes.contains_key(bus_id) {
            println!("Auto-Attach уже запущен для устройства {bus_id}");
            return Ok(());
        }

        match spawn_auto_attach(bus_id, &self.config.wsl_distro) {
            Ok(child) => {
                if !self.job.is_null() && self.job != INVALID_HANDLE_VALUE {
                    let handle = child.as_raw_handle();
                    unsafe {
                        if AssignProcessToJobObject(self.job, handle as *mut c_void) == 0 {
                            println!(
                                "Не удалось добавить процесс Auto-Attach в Job Object для {bus_id}"
                            );
                        }
                    }
                }

                self.auto_attach_processes.insert(bus_id.to_string(), child);
                if !self
                    .config
                    .auto_attach_devices
                    .contains(&bus_id.to_string())
                {
                    self.config.auto_attach_devices.push(bus_id.to_string());
                    save_config(&self.config);
                }
                Ok(())
            }
            Err(e) => {
                println!("Ошибка запуска Auto-Attach для {bus_id}: {e}");
                show_error(hwnd, &format!("Ошибка запуска Auto-Attach: {e}"));
                Err(())
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
        unsafe {
            if !self.job.is_null() && self.job != INVALID_HANDLE_VALUE {
                // KILL_ON_JOB_CLOSE terminates any remaining tree members.
                CloseHandle(self.job);
                self.job = ptr::null_mut();
            }
        }
    }
}

unsafe fn create_kill_on_close_job() -> winapi::um::winnt::HANDLE {
    let job = CreateJobObjectW(ptr::null_mut(), ptr::null());
    if job.is_null() {
        return ptr::null_mut();
    }

    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = mem::zeroed();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let ok = SetInformationJobObject(
        job,
        JobObjectExtendedLimitInformation,
        &mut info as *mut _ as *mut c_void,
        mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
    );
    if ok == 0 {
        CloseHandle(job);
        return ptr::null_mut();
    }
    job
}

fn main() {
    unsafe {
        let class_name: Vec<u16> = wide("USBIPD_GUI");
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

        let title = wide("USBIPD Manager");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
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

        let list_class = wide("LISTBOX");
        let hwnd_list = CreateWindowExW(
            0,
            list_class.as_ptr(),
            ptr::null(),
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | LBS_NOTIFY | LBS_HASSTRINGS,
            10,
            10,
            760,
            480,
            hwnd,
            ID_LIST as HMENU,
            h_instance,
            ptr::null_mut(),
        );
        if hwnd_list.is_null() {
            SetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA, 0);
            let _ = Box::from_raw(state_ptr);
            DestroyWindow(hwnd);
            ExitProcess(1);
        }

        let font: HFONT = GetStockObject(DEFAULT_GUI_FONT.try_into().unwrap()) as HFONT;
        SendMessageW(hwnd_list, WM_SETFONT, font as WPARAM, 1 as LPARAM);

        let warning_text = wide(
            "Примечание: USBdk или VPN могут повлиять на работу usbipd.\r\n\
             Рекомендуется отключить их при проблемах.\r\n\
             WSL-дистрибутив настраивается в config.json рядом с exe (поле wsl_distro).",
        );
        let static_class = wide("STATIC");
        let hwnd_static = CreateWindowExW(
            0,
            static_class.as_ptr(),
            warning_text.as_ptr(),
            WS_CHILD | WS_VISIBLE | SS_LEFT,
            10,
            500,
            760,
            55,
            hwnd,
            ID_STATIC as HMENU,
            h_instance,
            ptr::null_mut(),
        );
        SendMessageW(hwnd_static, WM_SETFONT, font as WPARAM, 1 as LPARAM);

        for (label, id, style) in [
            ("Bind", ID_BIND, BS_DEFPUSHBUTTON),
            ("Unbind", ID_UNBIND, BS_PUSHBUTTON),
            ("Attach", ID_ATTACH, BS_PUSHBUTTON),
            ("Detach", ID_DETACH, BS_PUSHBUTTON),
            ("Auto Attach", ID_AUTO_ATTACH, BS_PUSHBUTTON),
            ("Stop Auto-Attach", ID_STOP_AUTO, BS_PUSHBUTTON),
            ("Обновить", ID_REFRESH, BS_PUSHBUTTON),
        ] {
            create_button(hwnd, h_instance, label, id, style);
        }

        for id in [ID_BIND, ID_UNBIND, ID_ATTACH, ID_DETACH, ID_AUTO_ATTACH, ID_STOP_AUTO, ID_REFRESH]
        {
            let hwnd_button = GetDlgItem(hwnd, id);
            SendMessageW(hwnd_button, WM_SETFONT, font as WPARAM, 1 as LPARAM);
        }

        layout_controls(hwnd);

        {
            let state = &mut *state_ptr;
            state.restore_auto_attach(hwnd);
            populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices);
        }

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut msg: MSG = mem::zeroed();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

unsafe fn create_button(
    parent: HWND,
    h_instance: winapi::shared::minwindef::HINSTANCE,
    label: &str,
    id: i32,
    style: u32,
) {
    let class = wide("BUTTON");
    let text = wide(label);
    CreateWindowExW(
        0,
        class.as_ptr(),
        text.as_ptr(),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        100,
        40,
        parent,
        id as HMENU,
        h_instance,
        ptr::null_mut(),
    );
}

unsafe fn layout_controls(hwnd: HWND) {
    let mut rect = mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }

    let margin = 10;
    let button_h = 40;
    let button_row_gap = 10;
    let static_h = 55;
    let bottom_block = button_h * 2 + button_row_gap + static_h + margin * 3;
    let list_h = (height - bottom_block - margin).max(80);
    let list_w = (width - margin * 2).max(100);

    let hwnd_list = GetDlgItem(hwnd, ID_LIST);
    MoveWindow(hwnd_list, margin, margin, list_w, list_h, 1);

    let static_y = margin + list_h + margin;
    let hwnd_static = GetDlgItem(hwnd, ID_STATIC);
    MoveWindow(hwnd_static, margin, static_y, list_w, static_h, 1);

    let row1_y = static_y + static_h + margin;
    let row2_y = row1_y + button_h + button_row_gap;
    let btn_w = 100;
    let gap = 10;

    let row1 = [
        (ID_BIND, 0),
        (ID_UNBIND, 1),
        (ID_ATTACH, 2),
        (ID_DETACH, 3),
    ];
    for (id, idx) in row1 {
        let x = margin + idx * (btn_w + gap);
        MoveWindow(GetDlgItem(hwnd, id), x, row1_y, btn_w, button_h, 1);
    }

    let row2 = [
        (ID_AUTO_ATTACH, 130),
        (ID_STOP_AUTO, 150),
        (ID_REFRESH, 100),
    ];
    let mut x = margin;
    for (id, w) in row2 {
        MoveWindow(GetDlgItem(hwnd, id), x, row2_y, w, button_h, 1);
        x += w + gap;
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SIZE => {
            layout_controls(hwnd);
            0
        }
        WM_COMMAND => {
            let state_ptr =
                GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
            if state_ptr.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            // Borrow in place — do NOT Box::from_raw (re-entrancy / double-free risk).
            let state = &mut *state_ptr;
            let control_id = (wparam & 0xFFFF) as i32;
            let hwnd_list = GetDlgItem(hwnd, ID_LIST);

            match control_id {
                ID_BIND => handle_bind(hwnd, hwnd_list, state),
                ID_UNBIND => handle_unbind(hwnd, hwnd_list, state),
                ID_ATTACH => handle_attach(hwnd, hwnd_list, state),
                ID_DETACH => handle_detach(hwnd, hwnd_list, state),
                ID_AUTO_ATTACH => handle_auto_attach(hwnd, hwnd_list, state),
                ID_STOP_AUTO => handle_stop_auto_attach(hwnd, hwnd_list, state),
                ID_REFRESH => {
                    populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices)
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            let state_ptr =
                GetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA) as *mut AppState;
            SetWindowLongPtrW(hwnd, winapi::um::winuser::GWLP_USERDATA, 0);
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

fn handle_bind(hwnd: HWND, hwnd_list: HWND, state: &AppState) {
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
            wait_for_device_state(&bus_id, |s| !is_bindable_state(s));
            populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices);
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
            wait_for_device_state(&bus_id, is_bindable_state);
            populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices);
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
        Ok(()) => populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices),
        Err(err) => {
            println!("Ошибка attach: {err}");
            show_error(hwnd, &format!("Ошибка подключения: {err}"));
        }
    }
}

fn handle_detach(hwnd: HWND, hwnd_list: HWND, state: &AppState) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    match run_usbipd_detach(&bus_id) {
        Ok(()) => populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices),
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

    let _ = state.start_auto_attach(&bus_id, hwnd);
    populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices);
}

fn handle_stop_auto_attach(hwnd: HWND, hwnd_list: HWND, state: &mut AppState) {
    let Some(bus_id) = get_selected_device(hwnd_list) else {
        show_error(hwnd, "Устройство не выбрано");
        return;
    };

    state.stop_auto_attach(&bus_id);
    populate_usb_list(hwnd_list, hwnd, &state.config.auto_attach_devices);
}

/// Poll without dispatching nested window messages (avoids re-entrant WM_COMMAND).
fn wait_for_device_state(bus_id: &str, predicate: fn(&str) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Some(state)) = usbipd::get_device_state(bus_id) {
            if predicate(&state) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn populate_usb_list(hwnd_list: HWND, hwnd: HWND, auto_attach_devices: &[String]) {
    unsafe {
        SendMessageW(hwnd_list, LB_RESETCONTENT, 0, 0);

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
            let display_w = wide(&display);
            let result = SendMessageW(hwnd_list, LB_ADDSTRING, 0, display_w.as_ptr() as LPARAM);
            if result == -1 {
                println!("Ошибка добавления строки: {display}");
            }
        }

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
    let title = wide("Ошибка");
    let message_w = wide(message);
    unsafe {
        MessageBoxW(
            hwnd,
            message_w.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

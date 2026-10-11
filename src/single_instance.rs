use anyhow::{ensure, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, ReleaseMutex, SetEvent, WaitForSingleObject,
        EVENT_MODIFY_STATE,
    },
    UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, EnumWindows, GetLastActivePopup, GetPropW,
        GetWindowThreadProcessId, IsWindowVisible, RemovePropW, SetForegroundWindow, SetPropW,
    },
};

const NAME: &str = "Local\\CodexChatTransfer.Desktop.v1";
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Mutex(Handle);
impl Drop for Mutex {
    fn drop(&mut self) {
        unsafe {
            ReleaseMutex(self.0 .0);
        }
    }
}

pub struct Instance {
    _mutex: Mutex,
    _event: Handle,
    name: String,
}
pub struct Listener {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    name: String,
}
pub struct WindowMarker {
    hwnd: HWND,
    key: Vec<u16>,
}
impl Drop for WindowMarker {
    fn drop(&mut self) {
        unsafe {
            RemovePropW(self.hwnd, self.key.as_ptr());
        }
    }
}

impl Instance {
    pub fn acquire() -> Result<Option<Self>> {
        Self::acquire_named(NAME)
    }
    fn acquire_named(name: &str) -> Result<Option<Self>> {
        let mutex = Handle(unsafe { CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr()) });
        ensure!(
            !mutex.0.is_null(),
            "无法创建单实例锁：{}",
            std::io::Error::last_os_error()
        );
        match unsafe { WaitForSingleObject(mutex.0, 0) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                let mutex = Mutex(mutex);
                let event = Handle(unsafe {
                    CreateEventW(
                        std::ptr::null(),
                        0,
                        0,
                        wide(&format!("{name}.Activate")).as_ptr(),
                    )
                });
                ensure!(!event.0.is_null(), "无法创建窗口唤起事件");
                Ok(Some(Self {
                    _mutex: mutex,
                    _event: event,
                    name: name.to_owned(),
                }))
            }
            WAIT_TIMEOUT => {
                activate(name)?;
                Ok(None)
            }
            _ => anyhow::bail!("无法获取单实例锁：{}", std::io::Error::last_os_error()),
        }
    }

    pub fn register_window(&self, hwnd: HWND) -> Result<WindowMarker> {
        let key = wide(&format!("{}.Window", self.name));
        ensure!(
            unsafe { SetPropW(hwnd, key.as_ptr(), 1usize as HANDLE) } != 0,
            "无法登记应用窗口"
        );
        Ok(WindowMarker { hwnd, key })
    }

    pub fn listen(&self, callback: impl Fn() + Send + 'static) -> Listener {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let name = self.name.clone();
        let thread = thread::spawn(move || {
            let event = Handle(unsafe {
                CreateEventW(
                    std::ptr::null(),
                    0,
                    0,
                    wide(&format!("{name}.Activate")).as_ptr(),
                )
            });
            if event.0.is_null() {
                return;
            }
            while !thread_stop.load(Ordering::Acquire) {
                let result = unsafe { WaitForSingleObject(event.0, 200) };
                if thread_stop.load(Ordering::Acquire) {
                    break;
                }
                if result == WAIT_OBJECT_0 {
                    callback();
                } else if result != WAIT_TIMEOUT {
                    break;
                }
            }
        });
        Listener {
            stop,
            thread: Some(thread),
            name: self.name.clone(),
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let event = Handle(unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE,
                0,
                wide(&format!("{}.Activate", self.name)).as_ptr(),
            )
        });
        if !event.0.is_null() {
            unsafe {
                SetEvent(event.0);
            }
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Search {
    key: Vec<u16>,
    hwnd: HWND,
}
unsafe extern "system" fn find_window(hwnd: HWND, parameter: LPARAM) -> i32 {
    let search = unsafe { &mut *(parameter as *mut Search) };
    if !unsafe { GetPropW(hwnd, search.key.as_ptr()) }.is_null() {
        search.hwnd = hwnd;
        return 0;
    }
    1
}

fn activate(name: &str) -> Result<()> {
    // The second launch has foreground permission; grant it to the verified existing window owner.
    let mut search = Search {
        key: wide(&format!("{name}.Window")),
        hwnd: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(find_window), &mut search as *mut Search as LPARAM);
    }
    if !search.hwnd.is_null() {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(search.hwnd, &mut pid);
        }
        if pid != 0 {
            unsafe {
                AllowSetForegroundWindow(pid);
            }
        }
    }
    for _ in 0..40 {
        let event = Handle(unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE,
                0,
                wide(&format!("{name}.Activate")).as_ptr(),
            )
        });
        if !event.0.is_null() {
            ensure!(unsafe { SetEvent(event.0) } != 0, "无法唤起已有窗口");
            return Ok(());
        }
        thread::sleep(std::time::Duration::from_millis(50));
    }
    anyhow::bail!("已有实例正在启动或退出，请稍后重试")
}

pub fn focus_window(hwnd: HWND) {
    let popup = unsafe { GetLastActivePopup(hwnd) };
    let target = if !popup.is_null() && unsafe { IsWindowVisible(popup) } != 0 {
        popup
    } else {
        hwnd
    };
    unsafe {
        SetForegroundWindow(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_launch_notifies_existing_instance_and_lock_is_reusable() {
        let name = format!("Local\\CCT.Test.{}", uuid::Uuid::new_v4());
        let instance = Instance::acquire_named(&name).unwrap().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        let listener = instance.listen(move || {
            let _ = send.send(());
        });
        let duplicate_name = name.clone();
        assert!(
            thread::spawn(move || Instance::acquire_named(&duplicate_name).unwrap().is_none())
                .join()
                .unwrap()
        );
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        drop(listener);
        drop(instance);
        assert!(Instance::acquire_named(&name).unwrap().is_some());
    }
}

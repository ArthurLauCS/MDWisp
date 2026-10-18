//! System dialogs: open / save / pick folder (IFileDialog) and the
//! save-before-closing question (TaskDialog with real button labels).

use std::path::PathBuf;

use mdwisp_core::i18n::{t, tf};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::*;
use windows::core::*;

fn wide_z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn result_path(dlg: &IFileDialog) -> Option<PathBuf> {
    unsafe {
        let item = dlg.GetResult().ok()?;
        let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s.map(PathBuf::from)
    }
}

/// (label, "*.md;*.markdown") pairs.
fn set_filters(dlg: &IFileDialog, filters: &[(String, String)]) {
    let wide: Vec<(Vec<u16>, Vec<u16>)> = filters.iter().map(|(n, s)| (wide_z(n), wide_z(s))).collect();
    let specs: Vec<COMDLG_FILTERSPEC> = wide.iter().map(|(n, s)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(s.as_ptr()) }).collect();
    if !specs.is_empty() {
        unsafe {
            let _ = dlg.SetFileTypes(&specs);
        }
    }
}

pub fn open_file(owner: HWND, title: &str, filters: &[(String, String)], multi: bool) -> Vec<PathBuf> {
    unsafe {
        let Ok(dlg) = CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) else { return vec![] };
        let base: IFileDialog = dlg.cast().unwrap();
        let _ = base.SetTitle(PCWSTR(wide_z(title).as_ptr()));
        set_filters(&base, filters);
        if multi {
            let _ = base.SetOptions(base.GetOptions().unwrap_or_default() | FOS_ALLOWMULTISELECT | FOS_FILEMUSTEXIST);
        }
        if dlg.Show(Some(owner)).is_err() {
            return vec![];
        }
        let Ok(items) = dlg.GetResults() else { return vec![] };
        (0..items.GetCount().unwrap_or(0))
            .filter_map(|i| {
                let item = items.GetItemAt(i).ok()?;
                let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
                let s = p.to_string().ok();
                CoTaskMemFree(Some(p.0 as _));
                s.map(PathBuf::from)
            })
            .collect()
    }
}

pub fn pick_folder(owner: HWND, title: &str) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let base: IFileDialog = dlg.cast().ok()?;
        let _ = base.SetTitle(PCWSTR(wide_z(title).as_ptr()));
        let _ = base.SetOptions(base.GetOptions().unwrap_or_default() | FOS_PICKFOLDERS);
        dlg.Show(Some(owner)).ok()?;
        result_path(&base)
    }
}

pub fn save_file(owner: HWND, default_name: &str, filters: &[(String, String)]) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let base: IFileDialog = dlg.cast().ok()?;
        let _ = base.SetFileName(PCWSTR(wide_z(default_name).as_ptr()));
        set_filters(&base, filters);
        if let Some(ext) = std::path::Path::new(default_name).extension() {
            let _ = base.SetDefaultExtension(PCWSTR(wide_z(&ext.to_string_lossy()).as_ptr()));
        }
        dlg.Show(Some(owner)).ok()?;
        result_path(&base)
    }
}

pub fn markdown_filter() -> Vec<(String, String)> {
    vec![(t("Markdown 文档").into(), "*.md;*.markdown;*.mdx".into())]
}

#[derive(PartialEq, Eq, Debug)]
pub enum Choice {
    Save,
    Discard,
    Cancel,
}

/// "是否保存「name」的修改？" with Save / Don't save / Cancel.
pub fn confirm_save(owner: HWND, name: &str) -> Choice {
    let title = wide_z(t("保存文档"));
    let main = wide_z(&tf("是否保存「{0}」的修改？", &[&name]));
    let detail = wide_z(t("不保存会丢弃本次尚未写入文件的内容。"));
    let labels = [wide_z(t("保存")), wide_z(t("不保存"))];
    let buttons = [
        TASKDIALOG_BUTTON { nButtonID: 100, pszButtonText: PCWSTR(labels[0].as_ptr()) },
        TASKDIALOG_BUTTON { nButtonID: 101, pszButtonText: PCWSTR(labels[1].as_ptr()) },
    ];
    let cfg = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hwndParent: owner,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        dwCommonButtons: TDCBF_CANCEL_BUTTON,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(main.as_ptr()),
        pszContent: PCWSTR(detail.as_ptr()),
        cButtons: buttons.len() as u32,
        pButtons: buttons.as_ptr(),
        nDefaultButton: 100,
        ..Default::default()
    };
    let mut pressed = 0;
    unsafe {
        if TaskDialogIndirect(&cfg, Some(&mut pressed), None, None).is_err() {
            return Choice::Cancel;
        }
    }
    match pressed {
        100 => Choice::Save,
        101 => Choice::Discard,
        _ => Choice::Cancel,
    }
}

/// Open `url` (http, https, mailto) or a file with its default handler.
pub fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide_z(target).as_ptr()), None, None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}

/// Explorer with `path` selected.
pub fn reveal(path: &std::path::Path) {
    let arg = format!("/select,\"{}\"", path.display());
    unsafe {
        ShellExecuteW(None, w!("open"), w!("explorer.exe"), PCWSTR(wide_z(&arg).as_ptr()), None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    }
}

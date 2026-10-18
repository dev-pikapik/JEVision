//! The computer a run runs on, as its report records it.

use std::sync::OnceLock;

/// Whether this computer is a virtual machine; `None` where that can't be told.
pub fn is_virtual() -> Option<bool> {
    static VIRTUAL: OnceLock<Option<bool>> = OnceLock::new();
    *VIRTUAL.get_or_init(detect)
}

#[cfg(target_os = "macos")]
fn detect() -> Option<bool> {
    // Without `-n` each line is named, so a name an older macOS lacks can't shift the rest.
    let output = std::process::Command::new("/usr/sbin/sysctl")
        .args(["kern.hv_vmm_present", "hw.model"])
        .output()
        .ok()?;
    from_sysctl(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn detect() -> Option<bool> {
    None
}

/// Reads `sysctl` output. macOS sets `kern.hv_vmm_present` to 1 when it runs under a
/// hypervisor; a guest made with Apple's Virtualization framework also has a model like
/// `VirtualMac2,1`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn from_sysctl(output: &str) -> Option<bool> {
    let value = |name: &str| {
        output.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim())
        })
    };
    let model = value("hw.model");
    let hypervisor = value("kern.hv_vmm_present");
    if model.is_some_and(|m| m.starts_with("VirtualMac")) || hypervisor == Some("1") {
        return Some(true);
    }
    (hypervisor == Some("0")).then_some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_machines_are_told_by_the_hypervisor_flag_or_the_model() {
        let sysctl = |hv: &str, model: &str| {
            from_sysctl(&format!("kern.hv_vmm_present: {hv}\nhw.model: {model}\n"))
        };
        assert_eq!(sysctl("0", "Mac16,10"), Some(false));
        assert_eq!(sysctl("1", "VirtualMac2,1"), Some(true));
        assert_eq!(sysctl("1", "MacBookPro16,1"), Some(true));
        assert_eq!(sysctl("0", "VirtualMac2,1"), Some(true));
        // An older macOS without the hypervisor flag prints only the model.
        assert_eq!(from_sysctl("hw.model: VirtualMac2,1\n"), Some(true));
        assert_eq!(from_sysctl("hw.model: MacPro7,1\n"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_mac_is_known() {
        assert!(is_virtual().is_some());
    }
}

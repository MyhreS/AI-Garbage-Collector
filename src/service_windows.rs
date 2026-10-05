use crate::{
    config::home,
    platform::{powershell, ps_quote},
};
use anyhow::{Result, ensure};
const NAME: &str = "AI Garbage Collector";
pub fn installed() -> bool {
    powershell("Get-ScheduledTask -TaskName 'AI Garbage Collector' -ErrorAction Stop | Out-Null")
        .is_ok()
}
pub fn install() -> Result<()> {
    ensure!(
        std::env::var_os("AIGC_STATE_DIR").is_none(),
        "service installation does not support AIGC_STATE_DIR"
    );
    let exe = std::env::current_exe()?;
    powershell(&format!(
        "$a=New-ScheduledTaskAction -Execute {} -Argument 'collect' -WorkingDirectory {}; $hour=New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) -RepetitionInterval (New-TimeSpan -Hours 1); $login=New-ScheduledTaskTrigger -AtLogOn -User ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name); $p=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited; $s=New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 12); Register-ScheduledTask -TaskName '{}' -Action $a -Trigger @($hour,$login) -Principal $p -Settings $s -Force | Out-Null; Start-ScheduledTask -TaskName '{}'",
        ps_quote(&exe.to_string_lossy()),
        ps_quote(&home().to_string_lossy()),
        NAME,
        NAME
    ))?;
    Ok(())
}
pub fn uninstall() -> Result<()> {
    ensure!(
        std::env::var_os("AIGC_STATE_DIR").is_none(),
        "service removal does not support AIGC_STATE_DIR"
    );
    powershell(
        "$t=Get-ScheduledTask -TaskName 'AI Garbage Collector' -ErrorAction SilentlyContinue; if($t){Stop-ScheduledTask -InputObject $t; Unregister-ScheduledTask -InputObject $t -Confirm:$false}",
    )?;
    Ok(())
}
pub fn status() -> serde_json::Value {
    let loaded = installed();
    serde_json::json!({"installed":loaded,"loaded":loaded,"interval_seconds":3600,"scheduler":"Windows Task Scheduler","task":NAME})
}

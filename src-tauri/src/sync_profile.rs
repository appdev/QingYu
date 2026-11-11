use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use tauri::Manager;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    endpoint: String,
    bucket: String,
    region: String,
    prefix: String,
    access_key_id: String,
    secret_access_key: String,
}

impl Profile {
    fn validate(&self) -> Result<(), String> {
        // Incomplete inputs must also survive closing settings and restarting.
        if [
            &self.endpoint,
            &self.bucket,
            &self.region,
            &self.prefix,
            &self.access_key_id,
            &self.secret_access_key,
        ]
        .iter()
        .any(|value| value.len() > 4096 || value.chars().any(char::is_control))
        {
            return Err("连接字段过长或包含控制字符".into());
        }
        Ok(())
    }
}

fn load(path: &Path) -> Result<Option<Profile>, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("无法读取应用的 S3 配置文件".into()),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
        return Err("S3 配置文件无效，未覆盖原文件".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "无法打开 S3 配置文件")?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取 S3 配置文件")?;
    if bytes.len() > 65536 {
        return Err("S3 配置文件过大".into());
    }
    let profile: Profile =
        serde_json::from_slice(&bytes).map_err(|_| "S3 配置文件损坏，未覆盖原文件")?;
    profile.validate()?;
    Ok(Some(profile))
}

fn save(path: &Path, profile: &Profile) -> Result<(), String> {
    profile.validate()?;
    let value = serde_json::to_vec(profile).map_err(|_| "连接配置无效")?;
    let directory = path.parent().ok_or("应用配置目录无效")?;
    fs::create_dir_all(directory).map_err(|_| "无法创建应用配置目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法保护应用配置目录")?;
    }
    let mut temp =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| "无法创建 S3 配置临时文件")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| "无法保护 S3 配置文件")?;
    }
    temp.write_all(&value)
        .map_err(|_| "S3 配置写入失败，原文件未修改")?;
    temp.as_file()
        .sync_all()
        .map_err(|_| "S3 配置保存失败，原文件未修改")?;
    temp.persist(path)
        .map_err(|_| "无法替换 S3 配置文件，原文件未修改")?;
    Ok(())
}

fn authorize(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("连接配置仅由主窗口管理".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_load_profile(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Option<Profile>, String> {
    authorize(&window)?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|_| "无法打开应用配置目录")?
        .join("s3-connection.json");
    tauri::async_runtime::spawn_blocking(move || load(&path))
        .await
        .map_err(|_| "读取连接配置未完成".to_string())?
}

#[tauri::command]
pub async fn sync_save_profile(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    profile: Profile,
) -> Result<(), String> {
    authorize(&window)?;
    let path = app
        .path()
        .app_config_dir()
        .map_err(|_| "无法打开应用配置目录")?
        .join("s3-connection.json");
    tauri::async_runtime::spawn_blocking(move || save(&path, &profile))
        .await
        .map_err(|_| "保存连接配置未完成".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_complete_profiles_atomically_in_private_config_and_restores_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("app-config/s3-connection.json");
        assert!(load(&path).unwrap().is_none());
        let mut profile = Profile {
            endpoint: "https://unit.invalid".into(),
            bucket: "test".into(),
            region: "us-east-1".into(),
            prefix: "notes".into(),
            access_key_id: "synthetic-id".into(),
            secret_access_key: "synthetic-secret".into(),
        };
        save(&path, &profile).unwrap();
        assert_eq!(
            load(&path).unwrap().unwrap().secret_access_key,
            profile.secret_access_key
        );
        profile.secret_access_key = "new-synthetic-secret".into();
        save(&path, &profile).unwrap();
        assert_eq!(
            load(&path).unwrap().unwrap().secret_access_key,
            "new-synthetic-secret"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(path.parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let before = fs::read(&path).unwrap();
        profile.secret_access_key = "invalid\nvalue".into();
        assert!(save(&path, &profile).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, b"broken").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
    }

    #[test]
    fn allows_incomplete_profiles_but_rejects_invalid_fields() {
        let mut profile = Profile {
            endpoint: "".into(),
            bucket: "".into(),
            region: "us-east-1".into(),
            prefix: "miaoyan/v1".into(),
            access_key_id: "".into(),
            secret_access_key: "".into(),
        };
        assert!(profile.validate().is_ok());
        profile.secret_access_key = "synthetic\nsecret".into();
        assert!(profile.validate().is_err());
        profile.secret_access_key = "x".repeat(4097);
        assert!(profile.validate().is_err());
        assert!(serde_json::from_str::<Profile>("{}").is_err());
    }
}

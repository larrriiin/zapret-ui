fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut windows = tauri_build::WindowsAttributes::new();
        
        let manifest = r#"
            <assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
                <dependency>
                    <dependentAssembly>
                        <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" />
                    </dependentAssembly>
                </dependency>
                <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
                    <security>
                        <requestedPrivileges>
                            <requestedExecutionLevel level="asInvoker" uiAccess="false" />
                        </requestedPrivileges>
                    </security>
                </trustInfo>
            </assembly>
        "#;
        
        windows = windows.app_manifest(manifest);
        
        tauri_build::try_build(
            tauri_build::Attributes::new().windows_attributes(windows)
        ).expect("failed to run build script");
    } else {
        tauri_build::build();
    }
}

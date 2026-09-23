@echo off
rem TryingOpen2API 编译（Windows）
cargo build --release
if errorlevel 1 goto :err
echo 构建完成: target\release\tryingopen2api.exe
echo 运行: target\release\tryingopen2api.exe -- --config config.json  (或 Config::resolve 自动找 config.json)
goto :eof
:err
echo 构建失败
exit /b 1

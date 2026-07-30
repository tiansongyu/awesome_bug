SDL2 桌面蟑螂（Windows x64）

cockroach_overlay.exe
  单只蟑螂版。启动时检测当前屏幕分辨率，并随机选择初始位置。

cockroach_swarm_20.exe
  20 只蟑螂版。启动时检测当前屏幕分辨率，在不同区域随机生成
  20 只大小、速度略有差异的蟑螂。

程序默认速度倍率为 3。按 Ctrl+Alt+Q 可退出。
请保持 SDL2.dll 和 assets 文件夹与 EXE 位于同一目录。

桌面处于前台时，蟑螂只使用躯干碰撞体绕开图标及图标文字区域，
腿和触角不参与碰撞。拖拽桌面图标时，
蟑螂会避开正在移动的图标；即使图标直接覆盖蟑螂，也会继续移动脱离。
连续受阻时会主动探测多个方向并选择空隙脱困，不会长时间卡在图标夹角。
打开其他应用窗口后，不会把被应用遮住的桌面图标当作障碍物。

命令行示例：
  cockroach_overlay.exe --size 200
  cockroach_swarm_20.exe --speed 2
  cockroach_overlay.exe --help

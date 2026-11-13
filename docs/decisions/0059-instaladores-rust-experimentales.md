# ADR0059 · instaladores Rust separados del prototipo

Estado: aceptado para empaquetado de pruebas, sin promoción de paridad.

Usuario prioriza instaladores tras M2. Reemplazar Seiya.app/comando seiya antes de
M3 se descarta: rompería rollback y presentaría un launcher aún manual como app
completa. Elegimos paquete paralelo `seiya-rust-experimental`, comando seiya-rust
y app `Seiya Rust.app`, identificadores separados de Python. Sin hooks, servicios,
autostart, permisos automáticos, pesos, tokens o runtime de inferencia incluido.

Packaging continúa como herramienta Python de build; el payload Rust no incluye
servidor Python ni requiere Python para ejecutarse. UI HTML/JS embebida en el
binario Rust existente; no duplicar o rediseñar frontend. Binarios de release
proporcionados explícitamente, formatos/arquitectura verificados sin ejecutarlos.
No ejecutar compilación, descargar dependencias o hacer fallback dentro del
empaquetador. Allowlist de dos binarios: seiya-desktop y seiya-server.

macOS: bundle con binarios, nota, licencia y ejemplo de configuración. Launcher
abre Terminal y ayuda Rust, nunca --serve implícito; usuario prepara JSON/TLS y
ejecuta la entrada autorizada M2. No launcher gráfico/paridad acreditados.
Debian: paquete por arquitectura ELF, comandos CLI de chequeo/ayuda y API Rust;
host --serve macOS sigue no soportado. No fingir que .deb habilita MLX/imágenes.

CLI del empaquetador por defecto Rust exige bin-dir/arch y plataforma concreta;
prototipo conservado solo mediante --implementation python. Nombres de salidas
distintos y SHA256SUMS solo de los artefactos construidos en esa invocación, no
glob de dist que mezcle versiones/payloads previos. Nota/hashes dentro del DMG.

Migración: no tocar datos/configuración/modelos. Instalación y actualización reales
pendientes de autorización/dispositivo; no usar estos paquetes para retirar Python.
Rollback: seguir usando paquete/comando Python previo. No borrar locks, cuarentena
o historial al retirar binarios experimentales. Firma/notarización posteriores.

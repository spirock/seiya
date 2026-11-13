# ADR0060 · Windows x64 experimental, ZIP y setup.exe

Estado: aceptado para empaquetado/CI, no paridad desktop Windows.

Distribución Windows mediante instalador y .exe. Se reutilizan seiya-desktop (ayuda/versiones) y
seiya-server (API/config explícita), compilados Windows x86_64-pc-windows-msvc.
No portar MLX/Metal o anunciar imagen/broker nativo macOS. Config desktop v1 es
macOS/Unix; en Windows usar check-config de seiya-server con ejemplo discovery,
sin runtime. No añadir frontend simulado ni iniciar servidores al abrir instalador.

ZIP portable y setup.exe InnoSetup comparten allowlist/staging y hashes. Dos .exe,
nota Windows, ejemplo API sin credenciales/runtime, licencia/avisos, BUILD-INFO y
acceso de ayuda .cmd fijo. Validar PE32+ x64 y hashes sin ejecutar código de entrada.
Instalador per-user bajo LocalAppData/Programs, identidad separada del prototipo,
sin elevación, servicios, autostart, cambios de PATH, firewall o descarga de pesos.
Sin [Run], [Code], [Registry] o borrado recursivo de datos al desinstalar. Acceso
directo muestra ayuda y conserva consola, sin --serve implícito. Uninstaller estándar
solo para los archivos instalados; instalación/desinstalación reales no probadas.

Alternativas: solo ZIP es más simple, pero no satisface instalador si lo pide el
usuario; se entrega además como rollback/alternativa sin instalar. MSIX/WiX añade
otros requisitos de firma/SDK, innecesarios para este primer paquete. InnoSetup
viene en imagen Windows2025 (manifest oficial consultado); si no está disponible,
fallar esa tarea, no instalar silenciosamente con winget/chocolatey.

Rust compila con crt-static para no empaquetar/instalar redistribuibles VC++ por
sorpresa. CI ejecuta ayuda/version y check-config de los .exe extraídos de ZIP;
no inferencia, modelo, dispositivo personal o instalación silenciosa. Se inspecciona
staging y script del instalador; no afirmar extracción/instalación del setup por
compilarlo. Setup sin Authenticode/SmartScreen reputación; no pedir desactivar
protecciones del SO. Windows ARM64/x86 no son targets validados en este incremento.

Rollback: no instalar o retirar solo el programa experimental con desinstalador;
no tocar modelos/historial/configuración existentes. Pipeline Linux/macOS anterior
permanece separado; ningún cambio del servidor activo. M3 sigue pendiente.

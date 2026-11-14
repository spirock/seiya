# ADR0067 · plataforma nativa común y extensiones futuras

## Decisión implementable

Un driver Rust interno identifica OS/arquitectura del backend nativo. Consumidores
usan platform::current en lugar de repetir env::consts. Datos tipados e interfaz
Driver permiten fixtures de los tres SO; no se deduce el sistema del navegador,
User-Agent, páginas o argumentos del modelo. La detección es del target del
binario, no certificación de hardware ni detección del host tras una VM/contenedor.
Los cfg que compilan FFI/adaptadores exclusivos siguen siendo necesarios: no
simular disponibilidad de APIs Windows en macOS mediante un string.

Alternativas: detección JavaScript (identifica cliente, no host); comandos uname
o shell (innecesarios); repetir strings (divergencia). No cambian protocolo web,
rutas de perfiles, kernels, frontend ni proveedores. Rollback: consumidores
anteriores, sin migración de datos. Sin dependencia adicional.

## Operación y propuesta futura, NO implementada

La aplicación actual ejecuta Rust directamente y sirve HTML/CSS/JS al navegador;
no usa Docker como requisito de ejecución ni añade gestor de ventanas/webview.
Docker no prohíbe tools, pero su aislamiento dificulta acceso al escritorio y
dispositivos del anfitrión; no se adoptará para ocultar adaptadores pendientes.

Una futura carpeta de extensiones puede admitir manifiestos de herramientas
instaladas. Descubrimiento NO equivale a activación, instalación ni permisos.
Antes de implementar: contrato versionado, IDs únicos, entrypoint local acotado,
SO soportados, capacidades/permisos declarados, validación de rutas/symlinks,
límites de entrada/salida/tiempo, procedencia y revisión explícita del usuario.
No importar/ejecutar código al enumerar la carpeta. Una tool inválida debe
mostrarse rechazada, no provocar fallback ni ejecutarse para probarla.

Backend autentica y valida cada llamada; frontend/modelo solo proponen. Acciones
sensibles requieren confirmación host y permisos mínimos. Captura/visión,
accesibilidad, teclado/ratón, documentos y red son capacidades independientes,
con adaptadores nativos, auditoría y cancelación, no permisos implícitos por
instalar un archivo. Tools OpenCode siguen ejecutadas por OpenCode, no duplicadas.
Ruta definitiva, formato, sandbox y política de confianza siguen por diseñar;
no se crea ahora un loader de plugins ni control del PC.

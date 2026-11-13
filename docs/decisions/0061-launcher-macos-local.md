# ADR0061 · launcher nativo macOS y web local autenticada

Estado: implementación offline; no autorización de inferencia real del agente.

## Alternativas y decisión

Abrir Terminal con ayuda no cumple arranque sin comandos. Nuevo framework de UI
desktop añade dependencias/rediseño; una segunda UI HTTP de configuración añade
superficie de seguridad innecesaria. Se eligen diálogos del SO mediante osascript
con scripts constantes y datos por argv (sin shell, modelo o interpolación de paths).
Rust coordina configuración, validación, lifecycle y apertura del navegador.

`seiya-desktop --launch` (macOS) abre menú para configurar o elegir un perfil JSON.
Configuración guiada selecciona motores/almacenes ya existentes; mantiene tags y
hashes del ensayo, no promete motor/modelo intercambiable. Guardar requiere
confirmación; solo crea carpetas propias bajo HOME/Library/Application Support/Seiya.
Datos/coordination privados, validación de symlinks, perfil JSON con modo600,
escritura atómica y comparación previa contra baseline para no pisar cambios.
No toca permisos/archivos externos, no instala runtimes ni revisa cuarentenas.

Antes de arrancar muestra chequeo filesystem y pide Solo chat/Con opciones.
No arranque/inferencia por elegir archivo o guardar. La app mantiene control de
parada visible; cancelar/cerrar o expirar el host revoca admisión y espera cleanup.
Procesos de diálogo son propios, kill_on_drop y reaped, no matar procesos ajenos.

Uso local: listener exacto127.0.0.1:8015, HTTP solo loopback con Host/Origin exactos,
bootstrap de un uso, cookie HttpOnly/SameSite y prefix aleatorio existentes.
No wildcard, exposición LAN o certificados creados por sorpresa. LAN/TLS conserva
su entrada M2 y sus guardas. Ready transmite URL solo al launcher; open solo acepta
http/127.0.0.1:8015/session o trial sin credenciales URL/query/fragment.
No abrir navegador/consumir bootstrap antes de ready, ni repetirlo automáticamente
si open falla. Reabrir usa ruta autenticada, no recrea una sesión ni reenvía chat.

La app usa el mismo broker/settings/historial/UI. Loopback tiene otro origen que
LAN HTTPS: chats del navegador no se migran silenciosamente ni se declaran perdidos;
siguen en el origen anterior. Snapshots Rust se recuperan solo del data_dir elegido,
sin replay de trabajos. Inventario de paridad distingue implementado/mocks/real.

## Consecuencias y rollback

Perfil desktop v1 y --serve LAN no cambian de esquema. Windows/Linux no adquieren
launcher nativo por compartir binario. Instalador macOS invoca --launch en vez de
Terminal/--help, pero sin ejecutar app/instalador en CI. Tests usan UI/browser/host
simulados y directorios sintéticos; no osascript real, modelos o documentos del usuario.
Rollback: CLI M2 o Python anteriores, sin borrar perfiles/datos/modelos. No paridad
total ni app autónoma: runtimes compatibles existentes y guardas11434 se mantienen.

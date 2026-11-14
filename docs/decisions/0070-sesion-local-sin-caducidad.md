# ADR0070 · acceso local durante la vida de la app

La aplicación loopback usa /app/<namespace-aleatorio>/, con acceso inicial de un
uso /session/<token>. Cookie HttpOnly/SameSite=Strict sin Max-Age/Expires: no
caduca tras30min; el navegador puede retirarla al cerrar su sesión. No prometer
acceso persistente después de reiniciar app/navegador o borrar cookies. Clave y
namespace nuevos por host revocan el acceso anterior. Nunca cookie en localStorage.

El host nativo desktop no expira por tiempo total. Mantiene cierre explícito,
señales, memoria/propiedad, cuarentena y todos los deadlines por inferencia.
No perpetuar una operación activa ni retener pesos por mantener abierta la web.
Accesos LAN/TLS y ensayos manuales siguen temporales con namespace trial y
cookie1800s; el cambio no amplía exposición remota ni modifica OpenCode/API keys.

Alternativa descartada: cookie de años o eliminar auth. No resolvería reinicio
ni garantiza seguridad o continuidad. El namespace aleatorio no es un permiso;
se mantienen checks de cookie/Bearer/Host/Origin. Actualizar validación de links
de launcher/UI para app; no aceptar destinos arbitrarios, ni URL con query/hash.

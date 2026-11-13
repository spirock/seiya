# TLS fixture · no es una credencial

Certificado autofirmado y clave EC generados exclusivamente para tests sin red,
con CN/SAN `example.org`. **Clave deliberadamente pública: nunca usarla en un
servicio, dispositivo o configuración real.** No son credenciales del usuario.
DER codificado como hex para mantener fixtures textuales revisables.

Generado localmente con LibreSSL3.3.6: ecparam prime256v1, req -new -x509
-days3650, SAN DNS:example.org y basicConstraints CA:FALSE; key convertida con
pkcs8 -topk8 -nocrypt. Validez9 noviembre2026 a6 noviembre2036. Pruebas basadas
en fecha fija dentro de esa ventana, no evidencia de certificados de Internet.
No se añade este certificado al almacén de raíces de producción o del sistema.

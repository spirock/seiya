# Configuración desktop v1 · primer incremento offline

Contrato independiente de OpenAI, SessionRequest nativo y settings CAS de la web.
Ejemplo: `config.example.json`. Sus rutas son placeholders, no un entorno listo.
ADR0057 y `docs/APP_RUST.md` describen los límites de este incremento.

| Campo | Regla |
|---|---|
| version | Entero1 obligatorio; otra versión se rechaza |
| target | Solo `macos`; no activa backend macOS en otro SO |
| data_dir | Directorio existente, absoluto, canónico, privado |
| coordination_dir | Igual; separado de data_dir y dependencias |
| model_store | Directorio existente absoluto/canónico; no se recorren pesos |
| llm_binary | Archivo regular absoluto/canónico con algún bit ejecutable y sin escritura group/other |
| images | Opcional/null; objeto estricto binary y assets_dir |
| images.binary | Mismas reglas que llm_binary |
| images.assets_dir | Directorio existente absoluto/canónico; no valida manifiesto/pesos |
| features | Opcional; ausente significa images=false y multiagent=false |
| features.images / multiagent | Ambos bool obligatorios si features está presente |

Campos desconocidos/duplicados se rechazan también en objetos anidados. Sin
coerción, expansión de `~`/variables ni rutas por defecto. Config regular no
symlink y ≤65536bytes; JSON adicional tras el documento se rechaza. Datos y
coordinación no pueden ser ancestros/descendientes entre sí o de ninguna ruta
de dependencia, para evitar mezclar estado mutable con modelos/ejecutables.
Privado en Unix significa ningún bit group/other (`mode & 077 == 0`). Esto no
comprueba ACLs, identidad de propietario, capacidad efectiva de escritura ni
resuelve carreras de sustitución de archivos. No equivale a sandbox seguro.

Salida éxito JSON: version, target, target_matches_host, filesystem_checked=true,
requested_features, runtime_executed=false, inference_validated=false y
launcher_available=false. No emite paths/config crudo. Salida error por stderr:
`{"code":"canonical_path_required","field":"data_dir"}`. Códigos1 para error
de configuración y2 para uso CLI;0 solo metadata/estructura comprobada.

M2: `seiya-desktop --serve` consume el contrato con aprobación explícita y TLS/LAN,
solo macOS. coordination_dir debe coincidir adicionalmente con la autoridad
estable bajo HOME (ADR0058); el chequeo offline no acredita esa admisión.
Features inicializa un perfil nuevo, sin sustituir preferencias CAS guardadas.
CLI antigua de hosts no consume JSON. Véase APP_RUST para dependencias/pins y
puente de exclusión/cuarentena, restricciones de inventario y modo --chat-only.
launcher_available=false en el informe offline significa que no acredita un host
admitido/listo: no ejecuta los preflights operativos de --serve.

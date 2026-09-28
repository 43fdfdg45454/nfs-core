# Gateway: instalación

El gateway corre al lado de nfsd y deja que los clientes lleguen por una sola conexión QUIC. Cada
conexión TCP del cliente viaja como un stream HTTP/3 `CONNECT` y el gateway la entrega a nfsd
**desde la dirección real del cliente**. nfsd y `/etc/exports` deciden igual que sin gateway: el
gateway no tiene permisos propios ni ve el TLS del export.

## Requisitos del servidor

- Linux con nfsd (NFSv4.2) y, si los exports piden TLS, `tlshd`.
- Docker (o Podman) con la red del host y `CAP_NET_ADMIN`: el gateway conecta a nfsd con la IP del
  cliente (`IP_TRANSPARENT`) e instala las reglas de ruteo para que las respuestas de nfsd vuelvan
  a él.
- En el host, una vez (por ejemplo en `/etc/sysctl.d/90-nfs-gateway.conf`):

  ```
  # Los paquetes del gateway llegan a nfsd por lo con la dirección del cliente.
  net.ipv4.conf.all.rp_filter = 0
  net.ipv4.conf.lo.rp_filter = 0
  # Buffers UDP grandes para QUIC.
  net.core.rmem_max = 16777216
  net.core.wmem_max = 16777216
  ```

- Un puerto UDP para el gateway (en el ejemplo, 443/udp) abierto en el firewall, en la VPN o hacia
  Internet, según por dónde lleguen los clientes.

## Certificados

- **Del gateway:** un certificado de servidor cuyo SAN incluya el nombre o la IP que los clientes
  usan para llegar a él.
- **De los clientes (por defecto se exige):** el gateway solo acepta clientes con un certificado de
  la CA indicada. Puede ser la misma CA de los certificados de cliente de mTLS de los exports. Para
  no exigirlo (por ejemplo, detrás de una VPN): `NFS_GATEWAY_NO_CLIENT_AUTH=true`.

Con el volumen `/certs` y los nombres por defecto de la imagen, alcanza con tres archivos:
`gateway.pem`, `gateway.key` y `ca.pem`.

## Ejemplo con compose

Toda la configuración va por variables de entorno:

```yaml
services:
  nfs-gateway:
    image: ghcr.io/43fdfdg45454/nfs-gateway:latest
    network_mode: host
    cap_add: [NET_ADMIN]
    restart: unless-stopped
    volumes:
      - ./certs:/certs:ro
    # Opcionales (con sus valores por defecto):
    # environment:
      # NFS_GATEWAY_TARGET: 0.0.0.0:2049
      # NFS_GATEWAY_LISTEN: 0.0.0.0:443
      # NFS_GATEWAY_CERT: /certs/gateway.pem
      # NFS_GATEWAY_KEY: /certs/gateway.key
      # NFS_GATEWAY_CLIENT_CA: /certs/ca.pem
      # NFS_GATEWAY_NO_CLIENT_AUTH: "false"
      # NFS_GATEWAY_CONGESTION: bbr/1.5
      # NFS_GATEWAY_MARK: "0x4e46"
      # NFS_GATEWAY_TABLE: "100"
```

- **Sin `ports:`**: con `network_mode: host` Docker no mapea puertos (los ignora); el gateway
  escucha directo en la red del host. Para escuchar en una sola dirección, ponerla en
  `NFS_GATEWAY_LISTEN` (por ejemplo `198.51.100.1:4000`). Lo que hay que abrir en el firewall es
  el puerto **UDP** (QUIC va por UDP).
- **A dónde va cada túnel lo decide el gateway, no la app**: por defecto, a nfsd en este mismo host,
  en la dirección por la que llegó el cliente y el puerto 2049. Solo hace falta
  `NFS_GATEWAY_TARGET` si nfsd está en otro puerto (`0.0.0.0:<puerto>`) o en otra dirección.
  `0.0.0.0` y `127.0.0.1` significan "este host": el gateway conecta desde la dirección del
  cliente, y el kernel descarta un paquete hacia loopback con un origen externo, así que usa la
  dirección por la que llegó el cliente.
- En la app, un servidor QUIC lleva el nombre y el puerto **UDP** del gateway, nada de nfsd. Si el
  export pide TLS, el certificado de nfsd tiene que incluir en su SAN ese mismo nombre.
- Los certificados pueden montarse uno por uno (`./certs/ca.crt:/certs/ca.pem:ro`), siempre en
  PEM. `gateway.pem` es el certificado del gateway (con su SAN), y los intermedios si los hay.

## Variables

Cada una se puede dar también como argumento (`--listen`, `--target`, ...); si están las dos, gana
el argumento.

| Variable | Por defecto | Qué hace |
|---|---|---|
| `NFS_GATEWAY_TARGET` | `0.0.0.0:2049` | nfsd (dirección:puerto); `0.0.0.0` o `127.0.0.1`: este host, por la dirección que alcanzó el cliente. Solo se abren túneles hacia ahí. |
| `NFS_GATEWAY_LISTEN` | `0.0.0.0:443` | Dirección y puerto UDP donde escucha. |
| `NFS_GATEWAY_CERT`, `NFS_GATEWAY_KEY` | `/certs/gateway.pem`, `/certs/gateway.key` | Certificado y clave del gateway (PEM). |
| `NFS_GATEWAY_CLIENT_CA` | `/certs/ca.pem` | CA de los certificados de cliente exigidos. |
| `NFS_GATEWAY_NO_CLIENT_AUTH` | `false` | `true`: no exige certificado de cliente. |
| `NFS_GATEWAY_CONGESTION` | `bbr/1.5` | BBR con tope de 1,5 BDP en vuelo; `bbr` sin tope. |
| `NFS_GATEWAY_MARK` | `0x4e46` | Marca de socket de las reglas de ruteo. |
| `NFS_GATEWAY_TABLE` | `100` | Tabla de ruteo de esas reglas. |

## Exports

El gateway no cambia nada de `/etc/exports`: una línea por la dirección con la que el cliente
llega al gateway (la IP del túnel de la VPN, o la pública si llega por Internet). Si llega por
Internet, su dirección cambia al pasar de Wi-Fi a datos móviles, y cada dirección nueva se evalúa
como cualquier otra.

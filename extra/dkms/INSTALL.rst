Installation
------------

1. Extract the DKMS source tree into /usr/src:

    sudo tar -C /usr/src -xzf xt-geoip-3.30.tar.gz

   This creates:

       /usr/src/xt-geoip-3.30/

2. Register the module with DKMS:

    sudo dkms add -m xt-geoip -v 3.30

3. Build and install the module:

    sudo dkms build -m xt-geoip -v 3.30
    sudo dkms install -m xt-geoip -v 3.30

4. Load the module:

    sudo modprobe xt_geoip

5. Autoload the module at boot:

   echo xt_geoip | sudo tee /etc/modules-load.d/xt_geoip.conf >/dev/null


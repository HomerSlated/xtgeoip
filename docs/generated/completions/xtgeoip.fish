# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_xtgeoip_global_optspecs
    string join \n b/backup c/clean f/force l/legacy p/prune log-file= no-log config= ca-file= h/help V/version
end

function __fish_xtgeoip_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_xtgeoip_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_xtgeoip_using_subcommand
    set -l cmd (__fish_xtgeoip_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -l log-file -d 'Write the log to PATH, overriding [logging] in the config' -r
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -l config -d 'Read configuration from PATH instead of /etc/xtgeoip.conf' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -l ca-file -d 'Verify the MaxMind server against the CA bundle in PATH' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s b -l backup -d 'Back up current database before replacing it'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s c -l clean -d 'Delete current binary database files'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s f -l force -d 'Force the operation (overrides safety checks)'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s l -l legacy -d 'Enable legacy mode (historical compatibility only)'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s p -l prune -d 'Prune old bin archives (requires --backup)'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -l no-log -d 'Disable file logging, overriding [logging] in the config'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -s V -l version -d 'Print version'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -f -a "run" -d 'Fetch then build the full pipeline'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -f -a "build" -d 'Build binary database from local CSV archive'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -f -a "fetch" -d 'Download GeoLite2 CSV archive from MaxMind'
complete -c xtgeoip -n "__fish_xtgeoip_needs_command" -f -a "conf" -d 'Manage system configuration'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -l log-file -d 'Write the log to PATH, overriding [logging] in the config' -r
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -l config -d 'Read configuration from PATH instead of /etc/xtgeoip.conf' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -l ca-file -d 'Verify the MaxMind server against the CA bundle in PATH' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -s b -l backup -d 'Back up current database before replacing it'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -s c -l clean -d 'Delete current binary database files'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -s f -l force -d 'Force the operation (overrides safety checks)'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -s l -l legacy -d 'Enable legacy mode (historical compatibility only)'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -s p -l prune -d 'Prune old CSV archives after fetching'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -l no-log -d 'Disable file logging, overriding [logging] in the config'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand run" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -l log-file -d 'Write the log to PATH, overriding [logging] in the config' -r
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -l config -d 'Read configuration from PATH instead of /etc/xtgeoip.conf' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -l ca-file -d 'Verify the MaxMind server against the CA bundle in PATH' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -s b -l backup -d 'Back up current database before replacing it'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -s c -l clean -d 'Delete current binary database files'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -s f -l force -d 'Force the operation (overrides safety checks)'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -s l -l legacy -d 'Enable legacy mode (historical compatibility only)'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -s p -l prune -d 'Prune old bin archives after backup (requires --backup)'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -l no-log -d 'Disable file logging, overriding [logging] in the config'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand build" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -l log-file -d 'Write the log to PATH, overriding [logging] in the config' -r
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -l config -d 'Read configuration from PATH instead of /etc/xtgeoip.conf' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -l ca-file -d 'Verify the MaxMind server against the CA bundle in PATH' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -s p -l prune -d 'Prune old CSV archives after fetching'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -s l -l legacy
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -s b -l backup
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -s c -l clean
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -s f -l force
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -l no-log -d 'Disable file logging, overriding [logging] in the config'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand fetch" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -l log-file -d 'Write the log to PATH, overriding [logging] in the config' -r
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -l config -d 'Read configuration from PATH instead of /etc/xtgeoip.conf' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -l ca-file -d 'Verify the MaxMind server against the CA bundle in PATH' -r -F
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -s d -l default -d 'Show default (example) configuration'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -s s -l show -d 'Show system configuration'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -s e -l edit -d 'Open system configuration in $EDITOR'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -s c -l set-credentials -d 'Encrypt and store MaxMind account_id/license_key (#103)'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -l no-log -d 'Disable file logging, overriding [logging] in the config'
complete -c xtgeoip -n "__fish_xtgeoip_using_subcommand conf" -s h -l help -d 'Print help (see more with \'--help\')'

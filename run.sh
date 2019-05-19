#!/bin/bash
RSMIMIIO="./target/debug/rust-mimiio --token .token --input audio.raw"
$RSMIMIIO | jq -r 'reduce .response[].result as $r (""; . + $r)'


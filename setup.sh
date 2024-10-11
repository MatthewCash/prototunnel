#!/usr/bin/env bash

# Create namespaces

ip net a left
ip net a right

# Create veth pair (simple cable)

ip l a vleft ty veth peer n vright

ip l s vleft netns left
ip l s vright netns right

ip -n left l s vleft up
ip -n right l s vright up

# Assign IPs to each end of veth

ip -n left a a 192.168.40.1/24 dev vleft
ip -n right a a 192.168.40.2/24 dev vright

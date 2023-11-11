
# Tips

- Change the `log_level` in the Hermes config to increase detail

# Previously encountered bugs

- If basic operations like bank sending fails on a consumer chain with "tx contains unsupported message types", or you see messages involving expecting '0' gas prices, it is because the consumer has an ante handler (see app/consumer/ante in the respective repo) which disables certain message types before the first ICS packets are exchanged (Note: even if the channels have been opened, I have encountered horrendously difficult-to-debug cases where Hermes would fail to relay packets, and ICS would not have actually been initiated).

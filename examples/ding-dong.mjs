import { resolve } from 'node:path'
import { WechatyBuilder, ScanStatus } from 'wechaty'
import qrTerminal from 'qrcode-terminal'
import PuppetNeo from 'wechaty-puppet-neo'

const [card, peer, challenge] = process.argv.slice(2)
if (!card || !peer || !/^[a-zA-Z0-9-]{6,48}$/.test(challenge ?? '')) {
  console.error('Usage: node examples/ding-dong.mjs CARD_PATH EXACT_TEST_PEER FRESH_CHALLENGE')
  process.exit(2)
}
process.umask(0o077)
const bot = WechatyBuilder.build({ name: resolve(card), puppet: new PuppetNeo() })
let attempted = false

bot.on('scan', (code, status) => {
  if (code && status === ScanStatus.Waiting) qrTerminal.generate(code, { small: true })
  if (status === ScanStatus.Scanned) console.log('Scanned. Confirm the login on your phone.')
})
bot.on('login', () => console.log('WeChat accepted the session.'))
bot.on('ready', () => console.log(`Send "ding ${challenge}" from the selected test contact.`))
bot.on('error', error => console.error(error.message))
bot.on('message', async message => {
  if (attempted || message.self() || message.room() || message.talker().id !== peer || message.text() !== `ding ${challenge}`) return
  attempted = true
  console.log('Received the expected ding.')
  try {
    await message.say(`dong ${challenge}`)
    console.log('WeChat accepted the reply. Check it on the recipient device.')
  } catch (error) {
    console.error(error instanceof Error ? error.message : 'Reply failed.')
    process.exitCode = 1
  }
})
async function stop() {
  try {
    await bot.stop()
  } catch (error) {
    console.error(error instanceof Error ? error.message : 'Shutdown failed.')
    process.exitCode = 1
  }
}
process.once('SIGINT', stop)
process.once('SIGTERM', stop)
await bot.start()

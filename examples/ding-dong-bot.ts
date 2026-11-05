import { types } from 'wechaty-puppet'
import qrTerminal from 'qrcode-terminal'
import PuppetNeo from 'wechaty-puppet-neo'

const puppet = new PuppetNeo()

puppet.on('scan', ({ qrcode, status }) => {
  if (qrcode && status === types.ScanStatus.Waiting) {
    qrTerminal.generate(qrcode, { small: true })
  }
  console.info(`Login status: ${types.ScanStatus[status]}`)
})
puppet.on('login', () => console.info('Logged in.'))
puppet.on('ready', () => console.info('Ready. Send this account "ding" to receive "dong".'))
puppet.on('logout', () => console.info('Logged out.'))
puppet.on('error', ({ data }) => console.error(data))
puppet.on('message', async ({ messageId }) => {
  try {
    const message = await puppet.messagePayload(messageId)
    console.info(`Received: ${message.text ?? ''}`)
    if (message.type === types.Message.Text && !message.roomId &&
        message.talkerId !== puppet.currentUserId && message.text === 'ding') {
      await puppet.messageSendText(message.talkerId, 'dong')
      console.info('Sent: dong')
    }
  } catch (error) {
    console.error(error)
  }
})

process.once('SIGINT', () => { void puppet.stop().catch(console.error) })
process.once('SIGTERM', () => { void puppet.stop().catch(console.error) })
await puppet.start()

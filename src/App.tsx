import { MailScreen } from "./components/MailScreen";
import { MailProvider } from "./state/MailProvider";

export default function App() {
  return (
    <MailProvider>
      <MailScreen />
    </MailProvider>
  );
}
